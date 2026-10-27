#!/bin/sh
# #1733: eight invented projects, every task state present; isolated TASKQ_INSTALL.
set -eu
d=${1:?usage: fixture8.sh DIR}; mkdir -p "$d/install"; d=$(cd "$d" && pwd -P)
now=$(date +%s); : > "$d/install/projects"
i=0
for p in storefront billing-api mobile-app data-pipeline docs-site auth-service search infra; do
  mkdir -p "$d/$p/.taskq"; touch "$d/$p.log"; echo "$d/$p" >> "$d/install/projects"
  printf '{"board":"github","repo":"acme/%s"}\n' "$p" > "$d/$p/taskq.json"
  t=""; n=$((i*10+1))
  for s in doing review ask failed ready waiting later held; do
    t="$t{\"n\":$n,\"title\":\"$p task $n\",\"state\":\"$s\",\"labels\":[\"taskq\"]},"; n=$((n+1))
  done
  printf '{"project":"%s","host":"mac","version":"%s","issues":"https://github.com/acme/%s/issues","log":"%s","focus":["all"],"error":null,"request":null,"events_since":%s,"tasks":[%s]}\n' \
    "$p" "${TASKQ_VERSION:-0}" "$p" "$d/$p.log" $((now-86400)) "${t%,}" > "$d/$p/.taskq/status.json"
  printf 'since %s\n' $((now-86400)) > "$d/$p/.taskq/events"
  i=$((i+1))
done
