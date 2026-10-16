# Part sheets visualization with Grafana

The [`grafana`](../grafana) directory provides a Grafana datasource and a
dashboard showing the part sheets archived in ClickHouse:

* **Latest saves**: general and operation part sheets saved in the selected time
  range, refreshed every 5 seconds. Clicking a part ID shows its history.
* **Part history**: for the part selected in the `Part ID` variable, the fields
  that changed between successive saves of the general part sheet, every saved
  version, and the operation part sheets.

Station arrays (`SuivTravPiecParPost`, …) are displayed as one character per
station, first station on the left: `1` for true, `0` for false.

The procedure below targets a single Debian host running ClickHouse, the gateway
and Grafana. Run it from a checkout of this repository.

## ClickHouse read-only user

The [schema](../clickhouse/tables/traceability.sql) defines a
`traceability_viewer` role, allowed to read the part sheet tables only. Apply the
schema again if it predates this role (all statements are idempotent):

```sh
clickhouse client --param_database=traceability \
    --queries-file clickhouse/tables/traceability.sql
```

Create the user Grafana connects as, with a password of your choice:

```sql
CREATE USER IF NOT EXISTS grafana IDENTIFIED BY '…' DEFAULT DATABASE traceability;
GRANT traceability_viewer TO grafana;
```

## Grafana installation

Install Grafana from the official APT repository:

```sh
sudo apt-get install -y apt-transport-https gpg wget
sudo mkdir -p /etc/apt/keyrings
wget -q -O - https://apt.grafana.com/gpg.key \
    | gpg --dearmor | sudo tee /etc/apt/keyrings/grafana.gpg >/dev/null
echo "deb [signed-by=/etc/apt/keyrings/grafana.gpg]" \
    "https://apt.grafana.com stable main" \
    | sudo tee /etc/apt/sources.list.d/grafana.list
sudo apt-get update
sudo apt-get install -y grafana
```

Install the ClickHouse datasource plugin:

```sh
sudo grafana cli plugins install grafana-clickhouse-datasource
```

## Provisioning

Store the password of the ClickHouse `grafana` user in a file readable by Grafana
only:

```sh
sudo install -m 0640 -o root -g grafana /dev/null /etc/grafana/clickhouse-password
sudoedit /etc/grafana/clickhouse-password
```

Install the datasource, the dashboards provider and the dashboards:

```sh
sudo install -m 0644 grafana/provisioning/datasources/opcua-line-gateway.yaml \
    /etc/grafana/provisioning/datasources/
sudo install -m 0644 grafana/provisioning/dashboards/opcua-line-gateway.yaml \
    /etc/grafana/provisioning/dashboards/
sudo install -d -m 0755 /var/lib/grafana/dashboards/opcua-line-gateway
sudo install -m 0644 grafana/dashboards/*.json \
    /var/lib/grafana/dashboards/opcua-line-gateway/
```

The datasource expects ClickHouse on `127.0.0.1:8123` and the `traceability`
database: adapt the installed file if it differs.

Start Grafana:

```sh
sudo systemctl enable --now grafana-server
```

## Usage

Open <http://localhost:3000>, log in as `admin` / `admin` and set a new password
when prompted. The dashboard is in the `Traceability` folder, named
`Part sheets`.

Grafana listens on all interfaces by default. To restrict it to the local host,
set `http_addr = 127.0.0.1` in the `[server]` section of `/etc/grafana/grafana.ini`
and restart it.

## Updating the dashboard

The provisioned dashboard can't be saved from the UI. To change it, edit it in
the UI, export it as JSON (*Export* > *Export as JSON*, without *Export for
sharing externally*), replace `grafana/dashboards/part-sheets.json` with the
result, and install the file again. Grafana reloads dashboards files by itself.
