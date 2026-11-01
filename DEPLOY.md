# Setting up Tenseiga on another machine

This guide sets up both halves from scratch:

- **The server**, [Tenseiga Athena](https://github.com/Mythrax-ZS/tenseiga-athena): a fork of
  rAthena with the Samurai job and the Twin Flames Saga. It runs on Linux or Windows.
- **The client**, [Tenseiga](https://github.com/Mythrax-ZS/tenseiga): runs on Windows, Linux
  (and Steam Deck) or Android.

They can be on the same machine or on different ones. The client talks to the server over
four TCP ports:

| Port | Server | Needed for |
|---|---|---|
| 6900 | login-server | logging in |
| 6121 | char-server | character select |
| 5121 | map-server | the game |
| 8888 | web-server | guild emblems, the Adventurer's Agency |

The client only needs to know the login address. The login server tells it where the char
server is, and the char server tells it where the map server is. Those two addresses
(`char_ip`, `map_ip`) are the most common thing to get wrong; see [Network](#16-network).

---

## 1. The server

> **On OpenMediaVault 6?** Read [1.9](#19-on-openmediavault-6) first. It changes where the
> server goes, how the database runs, and how the firewall and autostart work.

### 1.1 Get the code

```
git clone https://github.com/Mythrax-ZS/tenseiga-athena.git
cd tenseiga-athena
```

The client's protocol version (`PACKETVER 20250402`) is already set in
`src/custom/defines_pre.hpp`. Don't pass `--enable-packetver` or `-DPACKETVER` when you build:
the header wins anyway. Don't use the Docker build in `tools/docker/` as it is either, because
it pins an older version (20211103).

### 1.2 The database (MariaDB)

Any MariaDB 10.5+ (or MySQL 8) works. Pick one way to run it.

**Docker** (Linux or Windows, simplest):

```
docker run -d --name ro-db --restart unless-stopped -p 127.0.0.1:3306:3306 \
  -e MARIADB_ROOT_PASSWORD=<root password> \
  -e MARIADB_DATABASE=ragnarok -e MARIADB_USER=ragnarok -e MARIADB_PASSWORD=<db password> \
  -v ro-db:/var/lib/mysql mariadb:11
```

The `127.0.0.1:` prefix keeps the database off the network; only the servers on this machine
reach it.

**Native:** on Debian/Ubuntu, `sudo apt install mariadb-server`. On Windows, use the MariaDB
installer from mariadb.org. Then create the database and user:

```sql
CREATE DATABASE ragnarok;
CREATE USER 'ragnarok'@'localhost' IDENTIFIED BY '<db password>';
GRANT ALL ON ragnarok.* TO 'ragnarok'@'localhost';
```

**Load the tables** (from the repo root). With Docker, replace `mariadb ...` with
`docker exec -i ro-db mariadb ...`:

```
mariadb -u ragnarok -p ragnarok < sql-files/main.sql
mariadb -u ragnarok -p ragnarok < sql-files/web.sql
mariadb -u ragnarok -p ragnarok < sql-files/roulette_default_data.sql
mariadb -u ragnarok -p ragnarok < sql-files/logs.sql
```

On Windows PowerShell, `<` doesn't work. Use `Get-Content sql-files\main.sql | mariadb ...`,
or run the files from HeidiSQL.

`sql-files/upgrades/` is only for databases made with an older version. `main.sql` is already
current, including the fork's guild emblem change (`upgrade_20261030_web.sql`).

**Change the server-to-server account.** `main.sql` creates account 1 as `s1` / `p1`, which the
char and map servers use to log in to the login server. Change it before you open any ports:

```sql
UPDATE login SET userid = '<server user>', user_pass = '<server password>' WHERE account_id = 1;
```

You'll put the same pair into the char and map configs in [1.4](#14-configure).

### 1.3 Build

The char and map servers must always be built from the same source: the Samurai's skills
change their shared character layout. Build everything together.

**Linux** (Debian/Ubuntu; other distributions have the same packages under similar names):

```
sudo apt install git make gcc g++ libmariadb-dev libmariadb-dev-compat zlib1g-dev libpcre3-dev
./configure
make clean server
```

This makes `login-server`, `char-server`, `map-server` and `web-server` in the repo root. It
also creates `conf/import/` and `db/import/` from their `-tmpl` folders.

**Windows:**

1. Install Visual Studio 2022 or newer (Community is fine) with the **Desktop development with
   C++** workload.
2. Open `rAthena.sln`, choose **Release** and **x64**, and run **Build > Build Solution**.

This puts `login-server.exe`, `char-server.exe`, `map-server.exe` and `web-server.exe` in the
repo root. The build also copies `libmysql.dll`, `pcre8.dll` and `zlib.dll` there, and creates
`conf/import\` and `db/import\` from their `-tmpl` folders.

To rebuild from the command line instead:

```
"C:\Program Files\Microsoft Visual Studio\<version>\Community\MSBuild\Current\Bin\MSBuild.exe" rAthena.sln -p:Configuration=Release -p:Platform=x64 -m
```

### 1.4 Configure

Never edit the files in `conf/` directly. Put your settings in `conf/import/`, which overrides
them and is not tracked by git. The build created these files; if one is missing, copy it from
`conf/import-tmpl/`.

**`conf/import/inter_conf.txt`**: how the servers reach the database. List all five
connections, even when they're the same:

```
login_server_ip: 127.0.0.1
login_server_id: ragnarok
login_server_pw: <db password>
login_server_db: ragnarok
ipban_db_ip: 127.0.0.1
ipban_db_id: ragnarok
ipban_db_pw: <db password>
ipban_db_db: ragnarok
char_server_ip: 127.0.0.1
char_server_id: ragnarok
char_server_pw: <db password>
char_server_db: ragnarok
map_server_ip: 127.0.0.1
map_server_id: ragnarok
map_server_pw: <db password>
map_server_db: ragnarok
web_server_ip: 127.0.0.1
web_server_id: ragnarok
web_server_pw: <db password>
web_server_db: ragnarok
log_db_ip: 127.0.0.1
log_db_id: ragnarok
log_db_pw: <db password>
log_db_db: ragnarok
```

**`conf/import/char_conf.txt`**:

```
userid: <server user>
passwd: <server password>
// The address players' clients connect to (see 1.6).
char_ip: <server address>
// "yes" makes every account set a 4-digit PIN. The client supports both.
pincode_enabled: no
```

**`conf/import/map_conf.txt`**:

```
userid: <server user>
passwd: <server password>
map_ip: <server address>
```

**`conf/import/login_conf.txt`** (optional):

```
// Lets players make an account by logging in as "name_M" or "name_F" with a new password.
new_account: yes
```

**`conf/import/battle_conf.txt`** (optional): rates and limits, such as `base_exp_rate`,
`job_exp_rate` and the `item_rate_*` keys. See `conf/battle/*.conf` for every setting. Without
it the server runs rAthena's defaults (1x).

> **Copying an existing server.** To make an exact copy of a server that's already set up,
> copy its `conf/import/` and `db/import/` folders over the new ones, then fix the addresses
> and passwords in them. `db/import/` holds that server's data overrides, such as its own
> level tables, and `conf/import/` its rates. Neither folder is in git.

### 1.5 Twin Flames maps

The saga's memorial dungeons use copies of stock maps under new names. The server needs
those copies in its map list. From the repo root (Python 3, no extra packages):

```
python3 tools/twin_flames_maps.py      # Windows: py tools\twin_flames_maps.py
```

This writes `db/import/map_cache.dat` and `db/import/map_index.txt` and adds the `tf_*` maps to
`conf/import/map_conf.txt`. Run it after the build has created `conf/import/`. Running it again
changes nothing. See `doc/custom_jobs/twin_flames.md` for details.

### 1.6 Network

`char_ip` and `map_ip` must be addresses the **players' clients** can reach:

- Everyone on the same machine: `127.0.0.1`.
- Players on your home network: the server's LAN address, e.g. `192.168.1.50`.
- Players over the internet: your public address or domain. Forward ports 6900, 6121, 5121
  and 8888 (TCP) on your router to the server.

A wrong `char_ip` shows as the client logging in, then hanging or disconnecting at character
select. A wrong `map_ip` shows as the same thing after picking a character.

Open the ports in the firewall:

```
# Linux with ufw (not OpenMediaVault, see 1.9)
sudo ufw allow 6900,6121,5121,8888/tcp

# Windows (PowerShell as administrator)
New-NetFirewallRule -DisplayName "Tenseiga Athena" -Direction Inbound -Protocol TCP -LocalPort 6900,6121,5121,8888 -Action Allow
```

Never forward or open port 3306 (the database).

### 1.7 Start and stop

**Linux:**

```
./athena-start start      # also: stop, restart, status
```

It starts all four servers in the background and writes their logs to `log/`.
`./athena-start watch 10` instead stays running and restarts any server that stops, checking
every 10 seconds; [1.9](#19-on-openmediavault-6) uses it to run the server as a service.

**Windows:** run `runserver.bat`. It opens a window for each server. You can also start
`login-server.exe`, `char-server.exe`, `web-server.exe` and `map-server.exe` yourself, in that
order.

The map server is ready once its window or log says `Server is 'ready'`. It takes a few minutes
the first time, because it loads every map and script.

### 1.8 Accounts and Game Masters

With `new_account: yes`, players make their own account by logging in as `name_M` (or `name_F`)
with the password they want. After that they log in as plain `name`. You can also make one
directly:

```sql
INSERT INTO login (userid, user_pass, sex, email) VALUES ('player', '<password>', 'M', 'a@a.com');
```

To make someone a Game Master, set their account's group to 99 (all GM commands). They need to
log in again for it to apply:

```sql
UPDATE login SET group_id = 99 WHERE userid = 'player';
```

### 1.9 On OpenMediaVault 6

OpenMediaVault (OMV) 6 is Debian 11 underneath, so the Linux steps above work, with these
differences. Run the commands as root over SSH (turn SSH on in the OMV web UI under
**Services > SSH**; OMV has no `sudo` by default, so drop it from the commands above).

**Put the server on the system disk, not a data drive.** OMV usually mounts data drives with
`noexec`, so programs on them won't start ("Permission denied"). Check a drive with
`findmnt -no OPTIONS /srv/dev-disk-by-uuid-...`. Use `/opt/tenseiga-athena` instead. The
database's files can live on a data drive, since they aren't programs.

**Run MariaDB in a container.** OMV's way to run extra services is Docker, through the
**Compose** plugin (install omv-extras, then **System > Plugins > openmediavault-compose**).
Add this as a new compose file under **Services > Compose > Files**, set the passwords and
the data path to a shared folder on a data drive, and start it:

```yaml
services:
  ro-db:
    image: mariadb:11
    container_name: ro-db
    restart: unless-stopped
    ports:
      - "127.0.0.1:3306:3306"
    environment:
      MARIADB_ROOT_PASSWORD: <root password>
      MARIADB_DATABASE: ragnarok
      MARIADB_USER: ragnarok
      MARIADB_PASSWORD: <db password>
    volumes:
      - /srv/dev-disk-by-uuid-<your drive>/appdata/ro-db:/var/lib/mysql
```

Then load the tables as in [1.2](#12-the-database-mariadb), with
`docker exec -i ro-db mariadb -u ragnarok -p'<db password>' ragnarok < sql-files/main.sql` and
so on.

**Build as its own user:**

```
apt install git make gcc g++ libmariadb-dev libmariadb-dev-compat zlib1g-dev libpcre3-dev python3
useradd --system --create-home --home-dir /opt/tenseiga-athena --shell /bin/bash tenseiga
su - tenseiga -c "git clone https://github.com/Mythrax-ZS/tenseiga-athena.git /opt/tenseiga-athena"
su - tenseiga -c "cd /opt/tenseiga-athena && ./configure && make clean server"
```

A NAS CPU can take half an hour or more for the build. Leave out `make -j`: each parallel job
needs 2 to 3 GB of memory for the map server.

Then configure ([1.4](#14-configure)) and add the saga maps ([1.5](#15-twin-flames-maps)) as the
`tenseiga` user: `su - tenseiga`, then `cd /opt/tenseiga-athena`.

**Start it with the NAS.** Save this as `/etc/systemd/system/tenseiga-athena.service`:

```ini
[Unit]
Description=Tenseiga Athena
After=network-online.target docker.service
Wants=network-online.target

[Service]
User=tenseiga
WorkingDirectory=/opt/tenseiga-athena
ExecStart=/opt/tenseiga-athena/athena-start watch 10
KillMode=control-group
TimeoutStopSec=60

[Install]
WantedBy=multi-user.target
```

```
systemctl daemon-reload
systemctl enable --now tenseiga-athena
systemctl status tenseiga-athena     # logs: /opt/tenseiga-athena/log/
```

`systemctl stop tenseiga-athena` stops all four servers; they save on the way out.

**Give the NAS a fixed address.** `char_ip` and `map_ip` are the NAS's LAN address, so set a
static one under **Network > Interfaces**, or reserve it on your router.

**Firewall:** don't install ufw on OMV. OMV's own firewall (**Network > Firewall**) is off
unless you've set it up. If you use it, add rules that accept TCP 6900, 6121, 5121 and 8888.
These ports don't clash with OMV's web UI (80/443) or Portainer (9000/9443).

---

## 2. The client

### 2.1 Download or build

Prebuilt packages are on the
[releases page](https://github.com/Mythrax-ZS/tenseiga/releases). Each build of `main` makes one:

| File | For |
|---|---|
| `tenseiga-windows-x86_64.zip` | Windows 10/11, 64-bit |
| `tenseiga-x86_64.AppImage` | Linux and Steam Deck |
| `tenseiga-android-arm64.apk` | Android (64-bit ARM) |

To build it yourself instead, install Rust (stable, from rustup.rs) and run:

```
git clone https://github.com/Mythrax-ZS/tenseiga.git
cd tenseiga
cargo build --release -p ro-client      # makes target/release/tenseiga(.exe)
```

On Linux, first install
`pkg-config libx11-dev libxkbcommon-dev libwayland-dev libxrandr-dev libxi-dev libxcursor-dev libxkbcommon-x11-dev libasound2-dev libudev-dev`.

Keep the `overlays/` folder next to the program (it's already in the zip). It holds the Samurai
and the saga's sprites, items and quests.

### 2.2 Game data

Tenseiga doesn't include Ragnarok Online's art, maps or sounds. It reads them from an installed
official client. Install the **iRO** (International) client from its official site. Tenseiga
uses its `data.grf`, `BGM/` and `System/` in place and never changes them.

Optional extras:

- **kRO (Korean) GRFs**, for newer content that iRO lacks. Add them to `grfs`, after iRO's.
- **English translation** ([ROenglishRE](https://github.com/llchrisll/ROenglishRE)): download it
  and add its `Translation/Renewal` folder to `overlays`.
- **Upscaled textures** (`tools/texpack`, see `docs/upscaling.md`): add the output folder to
  `overlays`.

### 2.3 Configure

The client reads `config/local.toml`. `config/example.toml` explains every key; the ones to set
are:

```toml
login_addr = "192.168.1.50"          # the server; ":6900" is added if no port is given
# web_addr = "192.168.1.50:8888"     # only if the web-server runs on another host
username = "player"                  # filled into the login screen
password = ""
client_dir = "C:/Gravity/Ragnarok Online"   # the iRO install
grfs = ["data.grf"]                  # leave out event.grf
# overlays = ["C:/ROenglishRE/Translation/Renewal"]
backend = "vulkan"                   # or dx12 / gl / auto if Vulkan won't start
```

In-game settings (graphics, sound, keys) are saved separately to `settings.toml`, next to
`local.toml`.

**Windows:** unzip anywhere. Copy `config\example.toml` to `config\local.toml` (the `config`
folder sits next to `tenseiga.exe`) and edit it. Then start `tenseiga.exe`.

**Linux / Steam Deck:**

```
chmod +x tenseiga-x86_64.AppImage
./tenseiga-x86_64.AppImage
```

The first start creates `~/.config/tenseiga/local.toml`. Edit it:
`client_dir` starts out as a Windows path, so point it at where you copied the iRO client,
e.g. `client_dir = "/home/deck/ro"`. On Steam Deck, add the AppImage to Steam as a
non-Steam game to play it in Game Mode.

**Android:**

1. Install the APK. It's sideloaded, so allow installing unknown apps when Android asks.
2. Start it once. That creates its folder,
   `Android/data/io.github.mythraxzs.tenseiga/files/`.
3. Over USB, copy the iRO client's `data.grf`, `BGM/` and `System/` into `files/ro/`. Optionally,
   put kRO's GRFs in `files/ro/kro/` and the English translation in `files/english/`.
4. Edit `files/config/local.toml` and set `login_addr` to the server.

The log is `files/tenseiga.log`.

### 2.4 If something goes wrong

The client logs to its console. On Windows it has none, so start it from a command prompt
with `tenseiga.exe > tenseiga.log 2>&1` to capture the log. On Linux, start the AppImage from a
terminal. On Android it's `files/tenseiga.log`.

| What you see | Likely cause |
|---|---|
| "server closed the connection" at login, or os error 10053 | Too many logins in a short time from one address: the server blocks it for 10 minutes (`conf/packet_athena.conf`). Wait, then try once. |
| Logs in, then stuck or disconnected at character select | `char_ip` is wrong, or port 6121 is blocked |
| Stuck or disconnected after picking a character | `map_ip` is wrong, or port 5121 is blocked |
| No map, missing textures or silence | `client_dir` or `grfs` is wrong; the log warns about GRFs and folders it couldn't open |
| Guild emblems or the Agency don't load | The web-server isn't running, or port 8888 is blocked |
| Asked for a PIN | `pincode_enabled` is on in the server config. Enter one, or turn it off |
| Window doesn't open | Try `backend = "dx12"` (Windows) or `"gl"` |
