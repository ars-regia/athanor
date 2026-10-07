#!/bin/bash
# Required health check (doc_recovery.md, R5): greetd, which runs the greeter, starts and stays
# up. A failure queues the previous deployment, so the check gives a slow first boot after an
# update two minutes, and asks one run of greetd to stay active for ten seconds, since a greeter
# in a crash loop is not a running one. It fails at once when systemd has given up on greetd
# (StartLimitBurst=3 within a minute, set by athanor-recovery).
set -euo pipefail

stable=0 seen=''
for _ in {1..120}; do
    state='' run=''
    while IFS='=' read -r key value; do
        case $key in
            ActiveState) state=$value ;;
            InvocationID) run=$value ;;
        esac
    done < <(systemctl show greetd.service --property=ActiveState,InvocationID)
    case $state in
        failed)
            echo "Greenboot check: greetd has failed."
            exit 1
            ;;
        active)
            if [[ $run == "$seen" ]]; then
                stable=$((stable + 1))
            else
                seen=$run stable=0
            fi
            if ((stable >= 10)); then
                echo "Greenboot check: greetd is running."
                exit 0
            fi
            ;;
        *) seen='' stable=0 ;;
    esac
    sleep 1
done
echo "Greenboot check: greetd did not stay up."
exit 1
