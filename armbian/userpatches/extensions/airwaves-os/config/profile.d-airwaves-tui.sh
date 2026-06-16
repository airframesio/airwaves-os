# Airwaves OS: launch the console TUI on the main VT (tty1) only. The serial
# console and SSH fall through to a normal shell. The TUI's "Open a shell"
# action sets AIRWAVES_TUI_SHELL=1 so the nested shell does not relaunch the TUI.
if [ -z "${AIRWAVES_TUI_SHELL:-}" ] && [ -x /opt/airwaves/scripts/airwaves-tui ]; then
    case "$(tty 2>/dev/null)" in
        /dev/tty1) exec /opt/airwaves/scripts/airwaves-tui ;;
    esac
fi
