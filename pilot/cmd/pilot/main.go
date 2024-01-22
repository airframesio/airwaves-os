package main

import (
	"os"
	"time"

	"pilot/lib/netplan"
	"pilot/lib/webui"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
)

const (
	netplanFile = "/etc/netplan/planewatcher.yaml"

	webUIListenAddr = ":80"
)

func fileExists(filename string) bool {
	info, err := os.Stat(filename)
	if os.IsNotExist(err) {
		return false
	}
	return !info.IsDir()
}

func main() {

	// set up logging
	log.Logger = log.Output(zerolog.ConsoleWriter{Out: os.Stderr, TimeFormat: time.UnixDate})
	zerolog.SetGlobalLevel(zerolog.DebugLevel)

	log := log.With().Str("listenAddr", webUIListenAddr).Logger()
	log.Info().Msg("started")

	// check if netplan file exists
	if !fileExists(netplanFile) {
		log.Debug().Str("netplan_config", netplanFile).Msg("generating firstrun config")
		err := netplan.DefaultConfig(netplanFile)
		if err != nil {
			panic(err)
		}
	}

	// start web ui
	webUI := webui.WebUI{
		ListenAddr:  webUIListenAddr,
		NetplanFile: netplanFile,
	}
	webUI.Run()

}
