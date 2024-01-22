package netplan

import (
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"time"

	"github.com/rs/zerolog/log"
	"github.com/vishvananda/netlink"
	"gopkg.in/yaml.v2"
)

type Netplan struct {
	Network Network `yaml:"network"`
}

type Network struct {
	Version   int                 `yaml:"version"`
	Renderer  string              `yaml:"renderer,omitempty"`
	Ethernets map[string]Ethernet `yaml:"ethernets,omitempty"`
}

type Ethernet struct {
	Interface `yaml:",inline"`
}

type Interface struct {
	Addresses []string `yaml:"addresses,omitempty"`
	// DHCP4 defaults to true, so we must use a pointer to know if it was specified as false
	DHCP4       *bool       `yaml:"dhcp4,omitempty"`
	DHCP6       *bool       `yaml:"dhcp6,omitempty"`
	Gateway4    string      `yaml:"gateway4,omitempty"`
	Nameservers Nameservers `yaml:"nameservers,omitempty"`
	MTU         int         `yaml:"mtu,omitempty"`
	Routes      []Route     `yaml:"routes,omitempty"`
}

type Route struct {
	From   string `yaml:"from,omitempty"`
	OnLink *bool  `yaml:"on-link,omitempty"`
	Scope  string `yaml:"scope,omitempty"`
	Table  *int   `yaml:"table,omitempty"`
	To     string `yaml:"to,omitempty"`
	Type   string `yaml:"type,omitempty"`
	Via    string `yaml:"via,omitempty"`
	Metric *int   `yaml:"metric,omitempty"`
}

type Nameservers struct {
	Search    []string `yaml:"search,omitempty,flow"`
	Addresses []string `yaml:"addresses,omitempty,flow"`
}

var (
	True  = bool(true)
	False = bool(false)

	ErrConfirmationTimeout = errors.New("timeout while waiting for confirmation")
	ErrTimeout             = errors.New("timeout")
)

// Load loads a netplan yaml file into a Netplan struct
func Load(filename string) (Netplan, error) {

	// open netplan yaml file
	f, err := os.Open(filename)
	if err != nil {
		return Netplan{}, err
	}

	// read netplan yaml file
	b, err := io.ReadAll(f)
	if err != nil {
		return Netplan{}, err
	}

	// unmarshal yaml to struct
	np := Netplan{}
	err = yaml.Unmarshal(b, &np)
	if err != nil {
		return Netplan{}, err
	}

	// return struct
	return np, nil
}

// Saves a Netplan object as YAML file filename
func (np *Netplan) Save(filename string) error {

	log := log.With().Str("filename", filename).Logger()

	// open netplan file
	f, err := os.Create(filename)
	if err != nil {
		return err
	}
	defer f.Close()

	// marshall netplan obj to yaml
	out, err := yaml.Marshal(&np)
	if err != nil {
		return err
	}

	// write output
	_, err = f.Write(out)
	if err != nil {
		return err
	}
	log.Info().Msg("wrote new netplan yaml")

	// chmod
	err = os.Chmod(filename, 0600)
	if err != nil {
		return err
	}
	log.Debug().Msg("chmodded netplan yaml to 0600")

	return nil
}

// WriteDefaultConfig writes a default netplan yaml config with dchp4 enabled for all detected interfaces
func DefaultConfig(filename string) error {

	// prep vars
	eths := make(map[string]Ethernet)

	// get ip link list
	ll, err := netlink.LinkList()
	if err != nil {
		return err
	}

	// for each link...
	for _, l := range ll {
		// if device (as opposed to bridge etc)
		if l.Type() == "device" {
			// if not loopback
			if !(l.Attrs().Flags&net.FlagLoopback == net.FlagLoopback) {
				// add interface
				eths[l.Attrs().Name] = Ethernet{
					Interface: Interface{
						DHCP4: &True,
					},
				}
			}
		}
	}

	// prep netplan obj for marshalling to yaml
	np := Netplan{
		Network: Network{
			Version:   2,
			Renderer:  "networkd",
			Ethernets: eths,
		},
	}

	// save netplan file
	err = np.Save(filename)
	if err != nil {
		return err
	}

	// apply netplan config
	err = ApplyImmediate()
	if err != nil {
		return err
	}

	return nil
}

// ApplyImmediate runs `netplan apply`
func ApplyImmediate() error {

	// prepare command
	c := exec.Command("netplan", "apply")
	log := log.With().Str("cmd", c.String()).Logger()

	// prepare stdout & stderr
	stderr, err := c.StderrPipe()
	if err != nil {
		return err
	}
	stdout, err := c.StdoutPipe()
	if err != nil {
		return err
	}

	// start process
	err = c.Start()
	if err != nil {
		return err
	}
	log = log.With().Int("pid", c.Process.Pid).Logger()

	// read stdout
	bStdout, err := io.ReadAll(stdout)
	if err != nil {
		return err
	}
	log = log.With().Str("stdout", string(bStdout)).Logger()

	// read stderr
	bStderr, err := io.ReadAll(stderr)
	if err != nil {
		return err
	}
	log = log.With().Str("stderr", string(bStderr)).Logger()

	// wait for execution to finish
	err = c.Wait()
	if err != nil {
		log.Err(err).Msg("error running netplan apply")
		return err
	}
	log.Debug().Msg("netplan apply succeeded")
	return nil
}

// ApplyImmediate runs `netplan apply`
func ApplyWithConfirmation(timeoutSecs uint) (confirmFunc func() error) {

	// prep output struct
	type output struct {
		bStdout, bStderr []byte
		err              error
	}

	// prep channels
	confirmChan := make(chan bool)
	outputChan := make(chan output)

	// prep output func
	confirmFunc = func() error {
		var o output

		// send confirmation
		confirmChan <- true

		// get output or timeout
		select {
		case <-time.After(time.Second):
			log.Err(ErrTimeout).Msg("timeout recv from outputChan")
			return ErrTimeout
		case o = <-outputChan:
		}

		// log context
		log := log.
			With().
			Str("stdout", string(o.bStdout)).
			Str("stderr", string(o.bStderr)).
			Logger()

		// log & return any errors
		if o.err != nil {
			log.Err(o.err).Msg("error")
		}
		return o.err
	}

	// run "netplan try" with timeout
	go func() {

		o := output{}

		// prepare command
		c := exec.Command("netplan", "try", "--timeout", fmt.Sprintf("%d", timeoutSecs))
		log := log.With().Str("cmd", c.String()).Logger()
		log.Debug().Msg("preparing to run command")

		// prepare stdin, stdout & stderr
		stdin, err := c.StdinPipe()
		if err != nil {
			log.Err(err).Msg("error opening stdin pipe")
			o.err = err
			outputChan <- o
			return
		}
		stderr, err := c.StderrPipe()
		if err != nil {
			log.Err(err).Msg("error opening stderr pipe")
			o.err = err
			outputChan <- o
			return
		}
		stdout, err := c.StdoutPipe()
		if err != nil {
			log.Err(err).Msg("error opening stdout pipe")
			o.err = err
			outputChan <- o
			return
		}

		// start process
		err = c.Start()
		if err != nil {
			log.Err(err).Msg("error starting command")
			o.err = err
			outputChan <- o
			return
		}
		log = log.With().Int("pid", c.Process.Pid).Logger()

		// wait for confirmation or timeout
		select {

		// handle timeout
		case <-time.After(time.Second * time.Duration(timeoutSecs)):

			// read stdout
			bStdout, err := io.ReadAll(stdout)
			if err != nil {
				log.Err(err).Msg("error reading stdout")
				o.err = err
				outputChan <- o
				return
			}
			log = log.With().Str("stdout", string(bStdout)).Logger()
			o.bStdout = bStdout

			// read stderr
			bStderr, err := io.ReadAll(stderr)
			if err != nil {
				log.Err(err).Msg("error reading stderr")
				o.err = err
				outputChan <- o
				return
			}
			log = log.With().Str("stderr", string(bStderr)).Logger()
			o.bStderr = bStderr

			// return error via chan
			log.Warn().Msg("did not receive confirmation, netplan will revert")
			o.err = ErrConfirmationTimeout
			outputChan <- o
			return

		// wait for confirm
		case <-confirmChan:
			log.Debug().Msg("received confirmation")
		}

		// send ENTER to confirm
		_, err = stdin.Write([]byte("\n"))
		if err != nil {
			log.Err(err).Msg("error sending ENTER to confirm")
			o.err = err
			outputChan <- o
			return
		}

		// read stdout
		bStdout, err := io.ReadAll(stdout)
		if err != nil {
			log.Err(err).Msg("error reading stdout")
			o.err = err
			outputChan <- o
			return
		}
		log = log.With().Str("stdout", string(bStdout)).Logger()
		o.bStdout = bStdout

		// read stderr
		bStderr, err := io.ReadAll(stderr)
		if err != nil {
			log.Err(err).Msg("error reading stderr")
			o.err = err
			outputChan <- o
			return
		}
		log = log.With().Str("stderr", string(bStderr)).Logger()
		o.bStderr = bStderr

		// wait for execution to finish
		err = c.Wait()
		if err != nil {
			log.Err(err).Msg("error running command")
			o.err = err
			outputChan <- o
			return
		}

		log.Debug().Msg("ran command")
		o.err = nil
		outputChan <- o
	}()

	return confirmFunc
}
