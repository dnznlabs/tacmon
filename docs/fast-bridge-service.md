# A fast bridge provider as a service

Tacit's own instructions, [Run one that stays on](https://github.com/src-company/tacit/blob/main/fastbridge-lp/README.md#run-one-that-stays-on),
get a provider running from a terminal with its data beside the code. `tacmon`'s defaults expect one step more:
the provider kept running by systemd, with its data in `/var/lib`. This is that step. It is one way to do it, not
the only one, and nothing here is part of Tacit.

Follow Tacit's instructions first, up to a `config.json` and a key, and start it once by hand to see it work.

## 1. A user, a data directory, a key file

Use a user that exists for this and nothing else. `lp` below.

```bash
sudo install -d -o lp -g lp -m 700 /var/lib/tacit-fastbridge-lp
sudo install -d -m 755 /etc/tacit-fastbridge-lp
sudo install -o lp -g lp -m 600 /dev/null /etc/tacit-fastbridge-lp/env
sudoedit /etc/tacit-fastbridge-lp/env
```

The file holds the key, and the alert webhook if you use one:

```
FB_LP_KEY=<the key>
FB_LP_ALERT_WEBHOOK=<optional>
```

Only that user can read it. The key is never in the unit file, the shell history or the config.

In the provider's `config.json`, set `"dataDir": "/var/lib/tacit-fastbridge-lp"` and leave `"port": 8787`.

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
- **To stop**, set the maximum amount to zero, wait for the open swaps to finish, then stop the unit. Tacit's
  instructions say how.
