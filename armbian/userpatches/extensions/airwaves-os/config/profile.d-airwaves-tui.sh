# Airwaves OS: launch the console TUI for the appliance operator.
#   - tty1: the autologin (root) console.
#   - SSH as the 'airwaves' user: drop straight into the console instead of a
#     shell. Run it via sudo so privileged actions (reboot, install, etc.) work;
#     fall back to an unprivileged console if passwordless sudo isn't available.
# The serial console and root SSH fall through to a normal shell. The TUI's
# "Open a shell" action sets AIRWAVES_TUI_SHELL=1 so the nested shell does not
# relaunch the TUI.
if [ -z "${AIRWAVES_TUI_SHELL:-}" ] && [ -x /opt/airwaves/scripts/airwaves-tui ]; then
    case "$(tty 2>/dev/null)" in
        /dev/tty1) exec /opt/airwaves/scripts/airwaves-tui ;;
    esac
    if [ -n "${SSH_CONNECTION:-}" ] && [ "$(id -un 2>/dev/null)" = "airwaves" ]; then
        if sudo -n -l /opt/airwaves/scripts/airwaves-tui >/dev/null 2>&1; then
            exec sudo /opt/airwaves/scripts/airwaves-tui
        else
            exec /opt/airwaves/scripts/airwaves-tui
        fi
    fi
fi
