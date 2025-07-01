WLED Backup

This project is for a command line program to detect and backup the configurations
for all [WLED][https://kno.wled.ge/] instances on your network.

It also contains the files to build a docker container to run that program for you on
a regular basis, and to have GitHub Actions publish that contain to Docker Hub as "dongar/wled-backup".

# Use the binarydirectly:

```
wled-backup --out-dir /backup/dir --search-secs 10
```

* --out-dir is the directory in which to store the backup files.
* --search-secs is how long to search your network for WLED MDNS advertisements.

# Deplay a docker image:

A sample compose.yaml file:

```
services:
  wled-backup:
    image: dongar/wled-backup:latest
    user: "1000:1000"
    restart: unless-stopped
    volumes:
      - /volume1/Backup/wled:/backup
    network_mode: host
```

These environmental variables are used by the docker image:

* BACKUP_DELAY is how long to wait between backups in bash "sleep". Default "1d".
* SEARCH_SECS How long to search your network. Default "10".

## Notes
I hit weird permission issues deplaying that on a Synology NAS trying to use
a normal user account. I had success using the built in account "SYSTEM" as "1:1".
If anyone can explain why I hit those issues, I'd appreciate it.
