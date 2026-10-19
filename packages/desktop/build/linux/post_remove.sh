#!/bin/sh
# FPM installs this directly as Debian postrm (remove/purge/upgrade), and
# embeds it in Arch post_remove() without arguments.
(
  set -eu
  case "${1:-remove}" in
    remove|purge) ;;
    *) exit 0 ;;
  esac

  manager=false
  if [ -d /run/systemd/system ]; then
    manager=true
  fi

  for unit in /etc/systemd/system/keeless-passkey@*.service; do
    [ -f "$unit" ] && [ ! -L "$unit" ] || continue
    name=${unit##*/}
    uid=${name#keeless-passkey@}
    uid=${uid%.service}
    case "$uid" in ''|*[!0-9]*) continue ;; esac

    # Fail before removing files if a live manager cannot stop the daemon.
    if "$manager"; then
      systemctl stop "$name"
      systemctl disable "$name"
      systemctl reset-failed "$name" || :
    fi
    for link in /etc/systemd/system/*.wants/"$name" /etc/systemd/system/*.requires/"$name"; do
      [ ! -L "$link" ] || rm -f -- "$link"
    done
    rm -f -- "$unit"
  done

  rm -f /usr/libexec/keeless/keeless-passkey-linux /etc/modules-load.d/keeless-uhid.conf
  if [ -d /usr/libexec/keeless ]; then
    rmdir /usr/libexec/keeless 2>/dev/null || :
  fi
  if "$manager"; then
    systemctl daemon-reload
  fi

  # Preserve the desktop cleanup normally supplied by Electron Builder.
  if command -v update-alternatives >/dev/null 2>&1; then
    update-alternatives --remove '${executable}' '/opt/${sanitizedProductName}/${executable}'
  else
    rm -f '/usr/bin/${executable}'
  fi
  profile='/etc/apparmor.d/${executable}'
  if [ -f "$profile" ]; then
    if command -v apparmor_parser >/dev/null 2>&1 &&
       command -v apparmor_status >/dev/null 2>&1 &&
       apparmor_status --enabled >/dev/null 2>&1; then
      apparmor_parser --remove "$profile" || :
    fi
    rm -f "$profile"
  fi
)
