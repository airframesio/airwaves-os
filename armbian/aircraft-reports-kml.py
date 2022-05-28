#!/usr/bin/env python3

import requests
import json
import simplekml

resp = requests.get('https://raw.githubusercontent.com/airframesio/data/master/json/noaa/aircraft_reports.json')
data = json.loads(resp.text)

kml = simplekml.Kml()

for item in data['aircraft_reports']:
  kml.newpoint(name=item['raw_text'], coords=[item['latitude'], item['longitude']])

print(kml.kml())
