# A fast bridge provider as a service

Tacit's own instructions, [Run one that stays on](https://github.com/src-company/tacit/blob/main/fastbridge-lp/README.md#run-one-that-stays-on),
get a provider running from a terminal with its data beside the code. `tacmon`'s defaults expect one step more:
the provider kept running by systemd, with its data in `/var/lib`. This is that step. It is one way to do it, not
the only one, and nothing here is part of Tacit.

Follow Tacit's instructions first, up to a `config.json` and a key, and start it once by hand to see it work.

## 1. A user, a data directory, a key file, the config

Use a user that exists for this and nothing else. `lp` below.

```bash
sudo install -d -o lp -g lp -m 700 /var/lib/tacit-fastbridge-lp
sudo install -d -o lp -g lp -m 700 /etc/tacit-fastbridge-lp
sudo install -o lp -g lp -m 600 /dev/null /etc/tacit-fastbridge-lp/env
sudo install -o lp -g lp -m 600 fastbridge-lp/config.json /etc/tacit-fastbridge-lp/config.json
sudoedit /etc/tacit-fastbridge-lp/env
```

The env file holds the key, the alert webhook if you use one, and where the config is:

```
FB_LP_KEY=<the key>
FB_LP_ALERT_WEBHOOK=<optional>
FB_LP_CONFIG=/etc/tacit-fastbridge-lp/config.json
```

Only that user can read it. The key is never in the unit file, the shell history or the config.

In `/etc/tacit-fastbridge-lp/config.json`, set `"dataDir": "/var/lib/tacit-fastbridge-lp"` and leave `"port": 8787`.

**Why the config goes there.** The provider reads one file: the path in `FB_LP_CONFIG`, or without it
`fastbridge-lp/config.json` under whatever directory it was started in. It looks nowhere else, and exits if the
file is missing. Beside the secrets, the config does not depend on the working directory and is out of the code
checkout. Remove or rename the copy in the checkout so nobody edits the one that is not read.

**Keep every setting in that file.** A `FB_LP_*` environment variable overrides the matching key without a
word, so a setting left in the env file makes a later edit to the config do nothing. The env file is for the
secrets and `FB_LP_CONFIG`. Both directions' terms belong in the config: the forward ones under `quote`, the
reverse ones under `reverse`, with `"enabled": true` to switch that side on.

## 2. The unit

`/etc/systemd/system/tacit-fastbridge-lp.service`:

```ini
[Unit]
Description=Tacit fast bridge liquidity provider
Wants=network-online.target
After=network-online.target

[Service]
User=lp
WorkingDirectory=/home/lp/tacit
EnvironmentFile=/etc/tacit-fastbridge-lp/env
ExecStart=/usr/bin/node fastbridge-lp/server.mjs
Restart=always
RestartSec=10
TimeoutStopSec=60
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
```

`WorkingDirectory` is the Tacit checkout and `ExecStart` the full path to `node` (`command -v node` as that user).

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now tacit-fastbridge-lp
journalctl -u tacit-fastbridge-lp -f      # the start line names the addresses to fund
```

## 3. tacmon

Run `tacmon` as the same user. With the unit named `tacit-fastbridge-lp`, the data in
`/var/lib/tacit-fastbridge-lp` and the port at 8787, it needs no configuration.

## Before you rely on it

- **The machine must stay up.** A provider that is off when a swap needs it loses that swap's funds. A laptop
  that sleeps, or a virtual machine on one, is not a place for this.
- **Back up the swap records**, to another disk: `make backup DEST=<dir>` in this repo, on a timer.
- **To change a setting**, edit `/etc/tacit-fastbridge-lp/config.json` and
  `sudo systemctl restart tacit-fastbridge-lp`. Look that no swap is open first, and read the terms back
  afterwards: `curl -s localhost:8787/info`, and `/reverse/info` with that side on.
- **To stop**, set the maximum amount to zero, wait for the open swaps to finish, then stop the unit. Tacit's
  instructions say how.
