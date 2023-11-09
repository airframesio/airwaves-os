#from rpardini and lanefu userpatches examples.

function post_family_tweaks_bsp__scrub_firstrun() {
	display_alert "removing bootsplash, autologin, firstrun from BSP" "${EXTENSION}" "warn"

	# Lets obliterate stuff in mass from the bsp, no autologin, no firstrun
	find "${destination}" -type f | grep \
		-e "bootsplash" \
		-e "autologin" \
		-e "firstrun" | xargs rm

	RM_OPTIONS="-f"

	##FIXME should we use `run_host_command_logged` here
	rm ${RM_OPTIONS} "${destination}"/etc/profile.d/armbian-check-first-login.sh
	rm ${RM_OPTIONS} "${destination}"/etc/updated-motd.d/10-armbian-header"
	rm ${RM_OPTIONS} "${destination}"/etc/lib/systemd/system/systemd-journald.service.d/override.conf
	rm ${RM_OPTIONS} "${destination}"/etc/lib/systemd/system/armbian-firstrun.service
	rm ${RM_OPTIONS} "${destination}"/lib/systemd/system/armbian-firstrun-config.service
	rm ${RM_OPTIONS} "${destination}"/lib/systemd/system/armbian-firstrun.service
	rm ${RM_OPTIONS} "${destination}"/lib/systemd/system/armbian-disable-autologin.service

	return 0
}

# Disable the Armbian onboarding stuff
function pre_customize_image__020_scrub_firstrun_contd() {
	display_alert "Hacking at the BSP" "${EXTENSION}" "info"
	# cleanup -- cloud-init makes some Armbian stuff actually get in the way
	[[ -f "${SDCARD}/boot/armbian_first_run.txt.template" ]] && rm -f "${SDCARD}/boot/armbian_first_run.txt.template"
	[[ -f "${SDCARD}/root/.not_logged_in_yet" ]] && rm -f "${SDCARD}/root/.not_logged_in_yet"

	# Enable motd generator, first-run will not run, so enable it directly here.
	run_host_command_logged chmod +x "${SDCARD}/etc/update-motd.d/"*

	return 0 # short-circuit above
}
