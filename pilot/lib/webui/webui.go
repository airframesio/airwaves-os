package webui

import (
	"bufio"
	_ "embed"
	"encoding/hex"
	"fmt"
	"html/template"
	"net"
	"net/http"
	"os"
	"pilot/lib/netplan"
	"strings"
	"time"

	"github.com/rs/zerolog/log"
	"github.com/vishvananda/netlink"
	"golang.org/x/sys/unix"
)

var (

	// html template for / (index)
	//go:embed index.html
	tmplIndexHTML string
	tmplIndex     = template.Must(template.New("index").Parse(tmplIndexHTML))

	// html template for /network
	//go:embed network.html
	tmplNetworkHTML string
	tmplNetwork     = template.Must(template.New("network").Parse(tmplNetworkHTML))

	// html template for when manual ip address applied
	//go:embed network_redirect.html
	tmplNetworkRedirectHTML string
	tmplNetworkRedirect     = template.Must(template.New("network_redirect").Parse(tmplNetworkRedirectHTML))

	// html template for when dhcp applied
	//go:embed network_todhcp.html
	tmplNetworkToDHCPHTML string
	tmplNetworkToDHCP     = template.Must(template.New("network_todhcp").Parse(tmplNetworkToDHCPHTML))

	// netplan yaml file path
	netplanFile string
)

type redirect struct {
	NewUrl string
}

type networkConfig struct {
	Netplan   netplan.Netplan
	Interface map[string]netiface

	Nameservers string
	Search      string
}

type netiface struct {
	IPv4Addr, IPv4Mask, IPv4Gateway string
	DHCPv4                          bool
}

// defines the configuration for the Web UI service
type WebUI struct {
	ListenAddr  string
	NetplanFile string
}

func handleIndex(w http.ResponseWriter, r *http.Request) {
	var err error

	reqTime := time.Now()

	log := log.With().
		Str("netplan_yaml", netplanFile).
		Str("uri", r.RequestURI).
		Str("src", r.RemoteAddr).
		Str("method", r.Method).
		Str("referrer", r.Referer()).
		Logger()

	err = tmplIndex.Execute(w, nil)
	if err != nil {
		log.Err(err).Msg("error executing template")
		w.WriteHeader(http.StatusInternalServerError)
		return
	}

	log.Debug().TimeDiff("rtt", time.Now(), reqTime).Msg("webui request")
}

func handleNetwork(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodPost:
		handleNetworkPOST(w, r)
	case http.MethodGet:
		handleNetworkGET(w, r)
	}
}

func handleNetworkPOST(w http.ResponseWriter, r *http.Request) {
	var (
		err error
		ip  net.IP
	)

	reqTime := time.Now()

	log := log.With().
		Str("netplan_yaml", netplanFile).
		Str("uri", r.RequestURI).
		Str("src", r.RemoteAddr).
		Str("method", r.Method).
		Str("referrer", r.Referer()).
		Logger()

	err = r.ParseForm()
	if err != nil {
		log.Err(err).Msg("error parsing form")
		w.WriteHeader(http.StatusBadRequest)
		return
	}

	// generate netplan yaml
	np, err := netplan.Load(netplanFile)
	if err != nil {
		log.Err(err).Msg("error loading netplan yaml")
		w.WriteHeader(http.StatusInternalServerError)
		return
	}

	// determine interface to update
	log = log.With().Str("iface", r.PostForm.Get("iface")).Logger()
	_, ok := np.Network.Ethernets[r.PostForm.Get("iface")]
	if !ok {
		log.Err(err).Msg("interface not found")
		w.WriteHeader(http.StatusBadRequest)
		return
	}

	newIntConf := netplan.Interface{}

	// depending on method...
	log = log.With().Str("ipv4.method", r.PostForm.Get("ipv4.method")).Logger()
	switch r.PostForm.Get("ipv4.method") {

	// if dhcp, then we just enable dhcp and that's it
	case "DHCP":
		newIntConf.DHCP4 = &netplan.True

	// if manual, a bit more complicated
	case "Manual":
		// disable dhcp
		newIntConf.DHCP4 = &netplan.False

		// check ipv4.address validity
		ip = net.ParseIP(r.PostForm.Get("ipv4.address"))
		log = log.With().Str("ipv4.address", ip.String()).Logger()
		if ip == nil {
			log.Err(err).Msg("invalid ipv4.address")
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// check ipv4.netmask validity
		mask := net.ParseIP(r.PostForm.Get("ipv4.netmask"))
		log = log.With().Str("ipv4.netmask", ip.String()).Logger()
		if mask == nil {
			log.Err(err).Msg("invalid ipv4.netmask")
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		mask4 := mask.To4()
		maskSz, _ := net.IPv4Mask(mask4[0], mask4[1], mask4[2], mask4[3]).Size()

		// get cidr notation for netplan
		_, addr, err := net.ParseCIDR(fmt.Sprintf("%s/%d", ip.String(), maskSz))
		if err != nil {
			log.Err(err).Msg("could not parse cidr")
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		log = log.With().Str("ipv4.cidr", addr.String()).Logger()

		// check ipv4.gateway validity
		gw := net.ParseIP(r.PostForm.Get("ipv4.gateway"))
		log = log.With().Str("ipv4.gateway", ip.String()).Logger()
		if gw == nil {
			log.Err(err).Msg("invalid ipv4.gateway")
			w.WriteHeader(http.StatusBadRequest)
			return
		}

		// add ip & default gw to interface config
		newIntConf.Addresses = []string{fmt.Sprintf("%s/%s", ip, strings.Split(addr.String(), "/")[1])}
		newIntConf.Routes = []netplan.Route{
			{
				To:  "default",
				Via: gw.String(),
			},
		}

		// add dns info to interface config
		newIntConf.Nameservers = netplan.Nameservers{
			Search:    strings.Fields(r.PostForm.Get("searchlist")),
			Addresses: strings.Fields(r.PostForm.Get("nameservers")),
		}

	default:
		log.Err(err).Msg("unknown ipv4.method")
		w.WriteHeader(http.StatusBadRequest)
		return
	}

	// update netplan file
	np.Network.Ethernets[r.PostForm.Get("iface")] = netplan.Ethernet{newIntConf}

	// save netplan yaml
	log = log.With().Str("filename", netplanFile).Logger()
	err = np.Save(netplanFile)
	if err != nil {
		log.Err(err).Msg("error writing netplan file")
		w.WriteHeader(http.StatusInternalServerError)
		return
	}

	// generate new url
	var newUrl string
	if r.URL.Port() == "" {
		newUrl = fmt.Sprintf("http://%s/network", ip)
	} else {
		newUrl = fmt.Sprintf("http://%s:%s/network", ip, r.URL.Port())
	}

	// apply netplan yaml after short delay
	go func() {
		time.Sleep(time.Microsecond * 2500)
		err := netplan.ApplyImmediate()
		if err != nil {
			log.Err(err).Msg("error applying netplan config")
			return
		}
	}()

	// return page to client
	switch r.PostForm.Get("ipv4.method") {
	case "Manual":
		rd := redirect{
			NewUrl: newUrl,
		}
		err = tmplNetworkRedirect.Execute(w, rd)
		if err != nil {
			log.Err(err).Msg("error executing template")
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
	case "DHCP":
		err = tmplNetworkToDHCP.Execute(w, nil)
		if err != nil {
			log.Err(err).Msg("error executing template")
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
	}

	log.Debug().TimeDiff("rtt", time.Now(), reqTime).Msg("webui request")

}

func handleNetworkGET(w http.ResponseWriter, r *http.Request) {
	var err error

	reqTime := time.Now()

	log := log.With().
		Str("netplan_yaml", netplanFile).
		Str("uri", r.RequestURI).
		Str("src", r.RemoteAddr).
		Str("method", r.Method).
		Str("referrer", r.Referer()).
		Logger()

	// prep network config
	nc := networkConfig{}
	nc.Interface = make(map[string]netiface)

	// load netplan yaml
	nc.Netplan, err = netplan.Load(netplanFile)
	if err != nil {
		log.Err(err).Msg("error loading netplan yaml")
		w.WriteHeader(http.StatusInternalServerError)
		return
	}

	// get information from running config for each iface
	for iface := range nc.Netplan.Network.Ethernets {
		log := log.With().Str("iface", iface).Logger()
		// get "live" network config for each interface
		l, err := netlink.LinkByName(iface)
		if err != nil {
			log.Err(err).Msg("error getting interface information from netlink")
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		addrs, err := netlink.AddrList(l, unix.AF_INET)
		if err != nil {
			log.Err(err).Msg("error getting interface addresses from netlink")
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		if len(addrs) < 1 {
			log.Error().Msg("no ipv4 addresses returned from netlink for interface")
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		if len(addrs) > 1 {
			log.Error().Msg("too many ipv4 addresses returned from netlink for interface")
			w.WriteHeader(http.StatusInternalServerError)
			return
		}

		// decode mask hex
		a, _ := hex.DecodeString(addrs[0].Mask.String()[0:2])
		b, _ := hex.DecodeString(addrs[0].Mask.String()[2:4])
		c, _ := hex.DecodeString(addrs[0].Mask.String()[4:6])
		d, _ := hex.DecodeString(addrs[0].Mask.String()[6:])
		mask := net.IPv4(a[0], b[0], c[0], d[0]).String()

		// get routes for interface
		var gw string
		routes, err := netlink.RouteList(l, unix.AF_INET)
		if err != nil {
			log.Err(err).Msg("too many ipv4 addresses returned from netlink for interface")
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		// find default gw in route list
		for _, route := range routes {
			if route.Dst == nil && route.Gw != nil {
				gw = route.Gw.String()
				break
			}
		}

		// prep interface
		nif := netiface{
			IPv4Addr:    addrs[0].IP.String(),
			IPv4Mask:    mask,
			IPv4Gateway: gw,
		}

		// dhcp
		if *nc.Netplan.Network.Ethernets[iface].DHCP4 == true {
			nif.DHCPv4 = true
		} else {
			nif.DHCPv4 = false
		}

		// add interface details
		nc.Interface[iface] = nif

	}

	// open resolv.conf for reading
	file, err := os.Open("/etc/resolv.conf")
	if err != nil {
		log.Err(err).Msg("couldn't open /etc/resolv.conf")
		w.WriteHeader(http.StatusInternalServerError)
		return
	}
	defer file.Close()

	// get dns info from running config
	nameservers := []string{}
	search := []string{}
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		if strings.Contains(scanner.Text(), "nameserver") {
			line := strings.Join(strings.Fields(scanner.Text()), " ")
			nameservers = append(nameservers, strings.Split(line, " ")[1])
		}
		if strings.Contains(scanner.Text(), "search") {
			line := strings.Join(strings.Fields(scanner.Text()), " ")
			search = strings.Split(line, " ")[1:]
		}
	}

	// store dns info
	nc.Nameservers = strings.Join(nameservers, " ")
	nc.Search = strings.Join(search, " ")

	// deal with scanner errors
	if err := scanner.Err(); err != nil {
		log.Err(err).Msg("couldn't open /etc/resolv.conf")
		w.WriteHeader(http.StatusInternalServerError)
		return
	}

	// execute config
	err = tmplNetwork.Execute(w, nc)
	if err != nil {
		log.Err(err).Msg("error executing template")
		w.WriteHeader(http.StatusInternalServerError)
		return
	}

	log.Debug().TimeDiff("rtt", time.Now(), reqTime).Msg("webui request")

}

func (conf *WebUI) Run() {
	var err error

	netplanFile = conf.NetplanFile

	// handle requests
	http.HandleFunc("/", handleIndex)
	http.HandleFunc("/network", handleNetwork)

	err = http.ListenAndServe(conf.ListenAddr, nil)
	if err != nil {
		panic(err)
	}

}
