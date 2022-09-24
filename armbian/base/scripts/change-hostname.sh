#!/usr/bin/env bash

current_hostname=$(cat /etc/hostname)
new_hostname=$1

#echo "Current hostname is $current_hostname"
#echo "New hostname is $new_hostname"

sudo sed -i "s/$current_hostname/$new_hostname/g" /etc/hosts
sudo sed -i "s/$current_hostname/$new_hostname/g" /etc/hostname
