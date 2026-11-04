#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLI="$ROOT/scripts/syntra.sh"

IMAGE="${SYNTRA_SMOKE_IMAGE:-syntra-engine:smoke}"
SKIP_BUILD="${SYNTRA_SMOKE_SKIP_BUILD:-0}"
DB_KIND="${SYNTRA_SMOKE_DB:-sqlite}"
PG_IMAGE="${SYNTRA_SMOKE_PG_IMAGE:-postgres:16-alpine}"
READY_TIMEOUT="${SYNTRA_SMOKE_READY_TIMEOUT:-120}"

RUN_ID="$$"
NAME="syntra-smoke-$RUN_ID"
PG_NAME="syntra-smoke-pg-$RUN_ID"
VOLUME="syntra-smoke-data-$RUN_ID"
SMALL_NAME="syntra-smoke-small-$RUN_ID"
SMALL_VOLUME="syntra-smoke-small-data-$RUN_ID"
NETWORK="syntra-smoke-net-$RUN_ID"
LOG_FILE="${TMPDIR:-/tmp}/syntra-smoke-$RUN_ID.log"

WORK=""
PORT=""
BASE=""
SMALL_BASE=""
WATCH_ID=""
COMPRESS_EVENTS_BEFORE=""
ADMIN_KEY=""
PASSED=0
FAILED=0
STARTED=$SECONDS
HTTP_STATUS=""
STEP_ERRS=()
ROWS=()
NET_ARGS=()

if [ -t 1 ]; then
  C_OK=$'\033[32m'
  C_BAD=$'\033[31m'
  C_OFF=$'\033[0m'
else
  C_OK=""
  C_BAD=""
  C_OFF=""
fi

pass() {
  PASSED=$((PASSED + 1))
  printf '%sPASS%s  %s\n' "$C_OK" "$C_OFF" "$1"
}

fail() {
  FAILED=$((FAILED + 1))
  printf '%sFAIL%s  %s\n' "$C_BAD" "$C_OFF" "$1"
  shift
  local line
  for line in "$@"; do
    printf '        %s\n' "$line"
  done
}

fatal() {
  fail "$@"
  exit 1
}

step_begin() {
  STEP_ERRS=()
}

step_end() {
  if [ "${#STEP_ERRS[@]}" -eq 0 ]; then
    pass "$1"
  else
    fail "$1" "${STEP_ERRS[@]}"
  fi
}

eq() {
  [ "$1" = "$2" ] || STEP_ERRS+=("$3: esperado '$2', obtido '$1'")
  return 0
}

match() {
  [[ "$1" =~ $2 ]] || STEP_ERRS+=("$3: obtido '$1'")
  return 0
}

chk() {
  local msg=$1
  shift
  if ! "$@" >/dev/null 2>&1; then
    STEP_ERRS+=("$msg")
  fi
  return 0
}

same_file() {
  cmp -s "$1" "$2"
}

nonneg() {
  awk -v v="$1" 'BEGIN { exit !(v + 0 >= 0) }'
}

at_most() {
  awk -v a="$1" -v b="$2" 'BEGIN { exit !(a + 0 <= b + 0) }'
}

fsize() {
  wc -c <"$1" | tr -d ' '
}

jget() {
  python3 -I - "$1" "$2" <<'PY'
import json
import sys

try:
    with open(sys.argv[1], "rb") as handle:
        value = json.loads(handle.read())
    for part in sys.argv[2].split("."):
        value = value[int(part)] if isinstance(value, list) else value[part]
except (KeyError, IndexError, ValueError, TypeError, OSError):
    print("")
    raise SystemExit(0)
if isinstance(value, bool):
    print("true" if value else "false")
elif value is None:
    print("null")
elif isinstance(value, (dict, list)):
    print(json.dumps(value))
else:
    print(value)
PY
}

jlen() {
  python3 -I - "$1" "$2" <<'PY'
import json
import sys

try:
    with open(sys.argv[1], "rb") as handle:
        value = json.loads(handle.read())
    for part in sys.argv[2].split("."):
        if part:
            value = value[int(part)] if isinstance(value, list) else value[part]
    print(len(value))
except (KeyError, IndexError, ValueError, TypeError, OSError):
    print(-1)
PY
}

usage_compress_events() {
  python3 -I - "$WORK/resp.body" <<'PY'
import json
import sys

try:
    with open(sys.argv[1], "rb") as handle:
        value = json.loads(handle.read())
    total = 0
    for bucket in value["totals"]["by_operation"]:
        if bucket["operation"] == "compress":
            total += bucket["events"]
    print(total)
except (KeyError, ValueError, TypeError, OSError):
    print(-1)
PY
}

ledger_violations() {
  python3 -I - "$WORK/resp.body" <<'PY'
import json
import sys

try:
    with open(sys.argv[1], "rb") as handle:
        value = json.loads(handle.read())
    checked = 0
    bad = 0
    for event in value["recent"]:
        operation = event["operation"]
        if operation not in ("decompress", "analyze", "read") or event["bytes_out"] <= 0:
            continue
        checked += 1
        if event["bytes_in"] <= 0:
            bad += 1
        if operation != "read" and event["bytes_out"] >= 100000 and event["cpu_ms"] <= 0:
            bad += 1
    print("%d %d" % (checked, bad))
except (KeyError, ValueError, TypeError, OSError):
    print("-1 -1")
PY
}

hdr() {
  awk -v want="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" '
    {
      line = $0
      sub(/\r$/, "", line)
      pos = index(line, ":")
      if (pos == 0) next
      key = tolower(substr(line, 1, pos - 1))
      if (key == want) {
        val = substr(line, pos + 1)
        sub(/^[ \t]+/, "", val)
        print val
        exit
      }
    }' "$WORK/resp.headers"
}

http() {
  local method=$1 path=$2
  shift 2
  local out=${HTTP_OUT:-$WORK/resp.body}
  : >"$WORK/resp.headers"
  HTTP_STATUS=$(curl -sS --max-time 300 -o "$out" -D "$WORK/resp.headers" \
    -w '%{http_code}' -X "$method" "$@" "$BASE$path") || HTTP_STATUS=000
}

auth_http() {
  local method=$1 path=$2
  shift 2
  http "$method" "$path" -H "x-api-key: $ADMIN_KEY" "$@"
}

upload() {
  local method=$1 path=$2 file=$3
  shift 3
  http "$method" "$path" -H 'content-type: application/octet-stream' --data-binary "@$file" "$@"
}

auth_upload() {
  local path=$1 file=$2
  shift 2
  upload POST "$path" "$file" -H "x-api-key: $ADMIN_KEY" "$@"
}

json_post() {
  local path=$1 body=$2
  shift 2
  http POST "$path" -H 'content-type: application/json' --data "$body" "$@"
}

container_running() {
  [ "$(docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null || echo false)" = "true" ]
}

cleanup() {
  local rc=$?
  trap - EXIT INT TERM
  if [ "$rc" -ne 0 ] || [ "$FAILED" -gt 0 ]; then
    if docker inspect "$NAME" >/dev/null 2>&1; then
      docker logs "$NAME" 2>&1 | sed $'s/\033\\[[0-9;]*m//g' >"$LOG_FILE" || true
      printf '\nlog do contêiner (últimas 60 linhas; completo em %s):\n' "$LOG_FILE"
      tail -n 60 "$LOG_FILE" | sed 's/^/    /'
    fi
  fi
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker rm -f "$SMALL_NAME" >/dev/null 2>&1 || true
  docker rm -f -v "$PG_NAME" >/dev/null 2>&1 || true
  docker volume rm -f "$VOLUME" >/dev/null 2>&1 || true
  docker volume rm -f "$SMALL_VOLUME" >/dev/null 2>&1 || true
  docker network rm "$NETWORK" >/dev/null 2>&1 || true
  if [ -n "$WORK" ] && [ -d "$WORK" ]; then
    rm -rf "$WORK"
  fi
  exit "$rc"
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

preflight() {
  local missing=() tool
  for tool in docker curl python3 awk cmp od head tr wc sed; do
    command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
  done
  if [ "${#missing[@]}" -gt 0 ]; then
    fatal "ferramentas ausentes: ${missing[*]}"
  fi
  if ! docker info >/dev/null 2>&1; then
    fatal "o daemon do Docker não responde (docker info falhou)"
  fi
  case "$DB_KIND" in
    sqlite | postgres) ;;
    *) fatal "SYNTRA_SMOKE_DB inválido: '$DB_KIND' (use sqlite ou postgres)" ;;
  esac
  WORK="$(mktemp -d "${TMPDIR:-/tmp}/syntra-smoke-work.XXXXXX")"
}

free_port() {
  python3 -I -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

random_hex() {
  od -An -tx1 -N"$1" /dev/urandom | tr -d ' \n'
}

build_image() {
  local start=$SECONDS
  if [ "$SKIP_BUILD" = "1" ]; then
    if docker image inspect "$IMAGE" >/dev/null 2>&1; then
      pass "imagem $IMAGE reaproveitada (SYNTRA_SMOKE_SKIP_BUILD=1)"
    else
      fatal "imagem $IMAGE não existe e SYNTRA_SMOKE_SKIP_BUILD=1"
    fi
    return 0
  fi
  if docker build -t "$IMAGE" "$ROOT" >"$WORK/build.log" 2>&1; then
    pass "build da imagem de produção $IMAGE ($((SECONDS - start)) s)"
  else
    printf '        últimas linhas do build:\n'
    tail -n 40 "$WORK/build.log" | sed 's/^/        /'
    fatal "build da imagem de produção $IMAGE"
  fi
}

start_postgres() {
  local pg_pass=$1
  docker network create "$NETWORK" >/dev/null
  NET_ARGS=(--network "$NETWORK")
  docker run -d --name "$PG_NAME" --network "$NETWORK" --label "syntra-smoke=$RUN_ID" \
    --tmpfs /var/lib/postgresql/data \
    -e POSTGRES_USER=syntra -e POSTGRES_PASSWORD="$pg_pass" -e POSTGRES_DB=syntra_smoke \
    "$PG_IMAGE" >/dev/null
  local deadline=$((SECONDS + 60))
  until docker exec "$PG_NAME" pg_isready -U syntra -d syntra_smoke >/dev/null 2>&1; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      fatal "Postgres não ficou pronto em 60 s"
    fi
    sleep 1
  done
  sleep 2
}

start_small_engine() {
  local port=$1
  docker volume create "$SMALL_VOLUME" >/dev/null
  docker run -d --name "$SMALL_NAME" --label "syntra-smoke=$RUN_ID" \
    -p "127.0.0.1:$port:3002" \
    -v "$SMALL_VOLUME:/data" \
    -e SYNTRA_DB_URL="sqlite:/data/small.db" \
    -e SYNTRA_API_KEYS="$ADMIN_KEY" \
    -e SYNTRA_MAX_BODY_MB=1 \
    -e SYNTRA_WATCH_ENABLED=false \
    -e RUST_LOG=engine=info \
    "$IMAGE" >/dev/null
}

wait_small_ready() {
  local deadline=$((SECONDS + READY_TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if [ "$(docker inspect -f '{{.State.Running}}' "$SMALL_NAME" 2>/dev/null || echo false)" != "true" ]; then
      return 1
    fi
    if curl -fsS --max-time 2 -o /dev/null "$SMALL_BASE/ready" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}

start_engine() {
  local db_url=$1
  docker volume create "$VOLUME" >/dev/null
  docker run -d --name "$NAME" --label "syntra-smoke=$RUN_ID" \
    ${NET_ARGS[@]+"${NET_ARGS[@]}"} \
    -p "127.0.0.1:$PORT:3002" \
    -v "$VOLUME:/data" \
    -e SYNTRA_DB_URL="$db_url" \
    -e SYNTRA_API_KEYS="$ADMIN_KEY" \
    -e SYNTRA_DEFAULT_EFFORT=balanced \
    -e RUST_LOG=engine=info \
    "$IMAGE" >/dev/null
}

wait_ready() {
  local deadline=$((SECONDS + READY_TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if ! container_running; then
      return 1
    fi
    if curl -fsS --max-time 2 -o /dev/null "$BASE/ready" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}

make_data() {
  awk 'BEGIN {
    split("sudeste sul norte nordeste centro-oeste", reg, " ")
    print "id,data,regiao,produto,valor,status"
    for (i = 0; i < 90000; i++) {
      printf "%d,2026-%02d-%02d,%s,produto-%d,%d.%02d,confirmado\n", i, (i % 12) + 1, (i % 28) + 1, reg[(i % 5) + 1], i % 60, i * 17, i % 100
    }
  }' >"$WORK/vendas.csv"

  awk 'BEGIN {
    split("api web batch auth pagamentos", svc, " ")
    split("info info info warn error", lvl, " ")
    for (i = 0; i < 30000; i++) {
      printf "{\"ts\":%d,\"nivel\":\"%s\",\"servico\":\"%s\",\"requisicao\":\"req-%07d\",\"latencia_ms\":%d,\"status\":%d}\n", 1790000000 + i * 3, lvl[(i % 5) + 1], svc[(i % 5) + 1], i, 5 + (i * 7) % 240, (i % 17 == 0) ? 500 : 200
    }
  }' >"$WORK/eventos.ndjson"

  head -c 2097152 /dev/urandom >"$WORK/aleatorio.bin"
  : >"$WORK/vazio.bin"
  printf 'A' >"$WORK/um-byte.bin"
}

declare -A OBJ_ID

exercise_file() {
  local name=$1
  local path="$WORK/$name"
  local size container=$WORK/$name.syntra
  size=$(fsize "$path")
  local before=$FAILED

  auth_upload "/api/v1/compress?filename=$name&effort=balanced" "$path"
  cp "$WORK/resp.body" "$container"
  local id plan savings verified klass essence dedup status=$HTTP_STATUS
  id=$(hdr x-syntra-id)
  plan=$(hdr x-syntra-plan)
  savings=$(hdr x-syntra-savings-pct)
  verified=$(hdr x-syntra-verified)
  klass=$(hdr x-syntra-content-class)
  essence=$(hdr x-syntra-essence-size)
  dedup=$(hdr x-syntra-deduplicated)
  OBJ_ID[$name]=$id

  step_begin
  eq "$status" 200 "status do compress"
  match "$id" '^[0-9a-f]{64}$' "x-syntra-id deve ser hash de 64 hex"
  match "$savings" '^-?[0-9]+(\.[0-9]+)?$' "x-syntra-savings-pct deve ser numérico"
  chk "x-syntra-plan ausente" test -n "$plan"
  eq "$verified" true "x-syntra-verified"
  eq "$dedup" false "x-syntra-deduplicated no primeiro envio"
  eq "$(hdr x-syntra-original-size)" "$size" "x-syntra-original-size"
  step_end "[$name] compress: 200, id, economia ${savings}%, plano ${plan:-?}, verificado"

  auth_upload "/api/v1/decompress" "$container"
  cp "$WORK/resp.body" "$WORK/$name.restored"
  step_begin
  eq "$HTTP_STATUS" 200 "status do decompress"
  eq "$(hdr x-syntra-fidelity-checked)" true "x-syntra-fidelity-checked"
  chk "reconstrução difere do original (cmp)" same_file "$path" "$WORK/$name.restored"
  step_end "[$name] decompress: bytes idênticos ao original (cmp)"

  auth_upload "/api/v1/compress?filename=$name&effort=balanced&response=json" "$path"
  step_begin
  eq "$HTTP_STATUS" 200 "status do reenvio"
  eq "$(jget "$WORK/resp.body" deduplicated)" true "deduplicated no reenvio"
  eq "$(jget "$WORK/resp.body" id)" "$id" "id no reenvio"
  eq "$(jget "$WORK/resp.body" references)" 2 "references no reenvio"
  step_end "[$name] reenvio: deduplicated=true, mesmo id, references=2"

  if [ "$name" = "aleatorio.bin" ]; then
    step_begin
    eq "$plan" stored "plano de dado aleatório"
    chk "essência ($essence) maior que o original ($size)" at_most "$essence" "$size"
    chk "economia negativa ($savings)" nonneg "$savings"
    step_end "[$name] dado aleatório vira stored sem expandir (payload $essence de $size B; container $(fsize "$container") B)"
  fi

  auth_http GET "/api/v1/objects/$id"
  step_begin
  eq "$HTTP_STATUS" 200 "status de GET /objects/:id"
  eq "$(jget "$WORK/resp.body" id)" "$id" "id"
  eq "$(jget "$WORK/resp.body" original_size)" "$size" "original_size"
  eq "$(jget "$WORK/resp.body" original_name)" "$name" "original_name"
  eq "$(jget "$WORK/resp.body" plan.label)" "$plan" "plan.label"
  eq "$(jget "$WORK/resp.body" references)" 2 "references"
  step_end "[$name] GET /objects/:id: metadados coerentes com o envio"

  HTTP_OUT="$WORK/$name.content" auth_http GET "/api/v1/objects/$id/content"
  local content_status=$HTTP_STATUS
  HTTP_OUT="$WORK/$name.essence" auth_http GET "/api/v1/objects/$id/essence"
  step_begin
  eq "$content_status" 200 "status de /content"
  chk "/content difere do original" same_file "$path" "$WORK/$name.content"
  eq "$HTTP_STATUS" 200 "status de /essence"
  chk "/essence difere do container devolvido no compress" same_file "$container" "$WORK/$name.essence"
  step_end "[$name] GET /content idêntico ao original e /essence idêntico ao container"

  local ok=sim
  [ "$FAILED" -eq "$before" ] || ok=NAO
  ROWS+=("$name|$size|$essence|$savings|${plan:-?}|$(fsize "$container")|$ok|$klass")
}

wait_for_object() {
  local search=$1 timeout=$2
  local deadline=$((SECONDS + timeout))
  while [ "$SECONDS" -lt "$deadline" ]; do
    auth_http GET "/api/v1/objects?search=$search"
    if [ "$HTTP_STATUS" = "200" ] && [ "$(jget "$WORK/resp.body" count)" -ge 1 ] 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}

phase_api() {
  local csv_id=${OBJ_ID[vendas.csv]}

  auth_http GET "/api/v1/objects?limit=100"
  step_begin
  eq "$HTTP_STATUS" 200 "status de GET /objects"
  eq "$(jget "$WORK/resp.body" count)" 5 "quantidade de objetos listados"
  step_end "GET /objects lista os 5 objetos enviados"

  auth_http GET "/api/v1/objects?search=vendas"
  step_begin
  eq "$(jget "$WORK/resp.body" count)" 1 "busca por nome 'vendas'"
  eq "$(jget "$WORK/resp.body" items.0.id)" "$csv_id" "id do item encontrado"
  step_end "GET /objects?search=vendas encontra pelo nome do arquivo"

  auth_upload "/api/v1/analyze?filename=vendas.csv&effort=balanced" "$WORK/vendas.csv"
  step_begin
  eq "$HTTP_STATUS" 200 "status do analyze"
  eq "$(jget "$WORK/resp.body" content_class)" tabular "content_class"
  eq "$(jget "$WORK/resp.body" detected_columns)" 6 "detected_columns"
  chk "analyze sem candidatos" test "$(jlen "$WORK/resp.body" candidates)" -ge 2
  eq "$(jget "$WORK/resp.body" recommended)" "$(jget "$WORK/resp.body" candidates.0.plan)" "recommended vs candidates[0]"
  step_end "POST /analyze: classe tabular, 6 colunas, candidatos ordenados, recomendado=$(jget "$WORK/resp.body" recommended)"

  auth_http GET "/api/v1/objects?limit=100"
  step_begin
  eq "$(jget "$WORK/resp.body" count)" 5 "objetos após analyze"
  step_end "analyze não gravou objeto algum"

  http GET "/api/v1/codecs" -H "x-api-key: $ADMIN_KEY"
  local names
  names=$(python3 -I -c 'import json,sys; print(" ".join(c["name"] for c in json.load(open(sys.argv[1]))["codecs"]))' "$WORK/resp.body" 2>/dev/null || echo "")
  step_begin
  eq "$HTTP_STATUS" 200 "status de /codecs"
  eq "$names" "stored lz4 deflate zstd bzip2 brotli xz" "catálogo de codecs"
  eq "$(jlen "$WORK/resp.body" transforms)" 4 "quantidade de transforms"
  eq "$(jlen "$WORK/resp.body" efforts)" 3 "quantidade de modos de esforço"
  step_end "GET /codecs: 7 codecs, 4 transforms, 3 modos de esforço"

  http GET "/health"
  step_begin
  eq "$HTTP_STATUS" 200 "status de /health"
  eq "$(jget "$WORK/resp.body" status)" ok "status"
  step_end "GET /health sem chave: 200 ok"

  http GET "/ready"
  step_begin
  eq "$HTTP_STATUS" 200 "status de /ready"
  eq "$(jget "$WORK/resp.body" ready)" true "ready"
  eq "$(jget "$WORK/resp.body" checks.database)" true "checks.database"
  eq "$(jget "$WORK/resp.body" checks.vault)" true "checks.vault"
  step_end "GET /ready sem chave: banco e vault ok"

  http GET "/metrics"
  local metrics_ok=0
  grep -Eq '^syntra_verify_failures_total 0$' "$WORK/resp.body" && metrics_ok=1
  local items
  items=$(awk '$1 == "syntra_items_total" { print $2 }' "$WORK/resp.body")
  step_begin
  eq "$HTTP_STATUS" 200 "status de /metrics"
  eq "$metrics_ok" 1 "linha 'syntra_verify_failures_total 0'"
  chk "syntra_items_total deveria ser >= 10 (obtido '$items')" test "${items:-0}" -ge 10
  step_end "GET /metrics sem chave: syntra_verify_failures_total 0, syntra_items_total=$items"

  http GET "/api/v1/openapi.json"
  step_begin
  eq "$HTTP_STATUS" 200 "status do openapi"
  eq "$(jget "$WORK/resp.body" openapi)" 3.0.3 "versão do OpenAPI"
  step_end "GET /api/v1/openapi.json sem chave: OpenAPI 3.0.3"

  http GET "/"
  step_begin
  eq "$HTTP_STATUS" 200 "status do dashboard"
  chk "dashboard não devolveu HTML" grep -qi '<html' "$WORK/resp.body"
  step_end "GET / (dashboard) sem chave: 200 HTML"

  auth_http GET "/api/v1/stats"
  step_begin
  eq "$HTTP_STATUS" 200 "status de /stats"
  eq "$(jget "$WORK/resp.body" database.backend)" "$DB_KIND" "database.backend"
  eq "$(jget "$WORK/resp.body" config.auth_enabled)" true "config.auth_enabled"
  eq "$(jget "$WORK/resp.body" process.throughput.verify_failures)" 0 "verify_failures"
  eq "$(jget "$WORK/resp.body" vault.objects_indexed)" 5 "vault.objects_indexed"
  step_end "GET /stats: backend $DB_KIND, autenticação ativa, 5 objetos no vault, 0 falhas de verificação"
}

phase_errors() {
  local csv_id=${OBJ_ID[vendas.csv]}

  upload POST "/api/v1/compress?filename=x.csv" "$WORK/um-byte.bin"
  step_begin
  eq "$HTTP_STATUS" 401 "compress sem chave"
  eq "$(jget "$WORK/resp.body" error.code)" unauthorized "error.code"
  step_end "POST /compress sem chave: 401 unauthorized"

  http GET "/api/v1/objects"
  step_begin
  eq "$HTTP_STATUS" 401 "listagem sem chave"
  step_end "GET /objects sem chave: 401"

  http GET "/api/v1/objects" -H "x-api-key: chave-errada-$RUN_ID"
  step_begin
  eq "$HTTP_STATUS" 401 "listagem com chave errada"
  step_end "GET /objects com chave errada: 401"

  http GET "/api/v1/objects" -H "authorization: Bearer $ADMIN_KEY"
  step_begin
  eq "$HTTP_STATUS" 200 "autenticação por Bearer"
  step_end "Authorization: Bearer <chave> também autentica"

  local bad
  step_begin
  for bad in zzz "$(printf 'f%.0s' $(seq 1 63))" "$(printf 'g%.0s' $(seq 1 64))"; do
    auth_http GET "/api/v1/objects/$bad"
    eq "$HTTP_STATUS" 400 "id malformado '${bad:0:20}' (status)"
    eq "$(jget "$WORK/resp.body" error.code)" invalid_object_id "id malformado '${bad:0:20}' (error.code)"
  done
  step_end "id malformado (curto, 63 hex, 64 não-hex): 400 invalid_object_id"

  auth_http GET "/api/v1/objects/$(printf 'a%.0s' $(seq 1 64))"
  step_begin
  eq "$HTTP_STATUS" 404 "id bem formado e inexistente"
  eq "$(jget "$WORK/resp.body" error.code)" not_found "error.code"
  step_end "id bem formado e inexistente: 404 not_found"

  auth_upload "/api/v1/compress?codec=xz&level=99" "$WORK/um-byte.bin"
  step_begin
  eq "$HTTP_STATUS" 400 "nível fora da faixa"
  eq "$(jget "$WORK/resp.body" error.code)" invalid_level "error.code"
  step_end "nível fora da faixa do codec: 400 invalid_level"

  auth_upload "/api/v1/compress?effort=turbo" "$WORK/um-byte.bin"
  step_begin
  eq "$HTTP_STATUS" 400 "esforço inválido"
  eq "$(jget "$WORK/resp.body" error.code)" invalid_effort "error.code"
  step_end "esforço inválido: 400 invalid_effort"

  auth_upload "/api/v1/decompress" "$WORK/vazio.bin"
  step_begin
  eq "$HTTP_STATUS" 400 "decompress com corpo vazio"
  eq "$(jget "$WORK/resp.body" error.code)" empty_body "error.code"
  step_end "decompress com corpo vazio: 400 empty_body"

  head -c 4096 /dev/urandom >"$WORK/lixo.bin"
  auth_upload "/api/v1/decompress" "$WORK/lixo.bin"
  step_begin
  eq "$HTTP_STATUS" 400 "decompress de lixo"
  chk "corpo de erro sem error.code" test -n "$(jget "$WORK/resp.body" error.code)"
  step_end "decompress de bytes que não são container: 400 $(jget "$WORK/resp.body" error.code)"

  python3 -I - "$WORK/vendas.csv.syntra" "$WORK/vendas.corrompido.syntra" <<'PY'
import sys

with open(sys.argv[1], "rb") as src:
    data = bytearray(src.read())
data[len(data) // 2] ^= 0xFF
with open(sys.argv[2], "wb") as dst:
    dst.write(data)
PY
  auth_upload "/api/v1/decompress" "$WORK/vendas.corrompido.syntra"
  step_begin
  eq "$HTTP_STATUS" 422 "container com 1 byte alterado"
  eq "$(jget "$WORK/resp.body" error.code)" envelope_corrupted "error.code"
  chk "erro sem dica (hint)" test -n "$(jget "$WORK/resp.body" error.hint)"
  step_end "container com 1 byte alterado: 422 envelope_corrupted, com hint, nenhum dado devolvido"

  auth_upload "/api/v1/inspect" "$WORK/vendas.csv.syntra"
  step_begin
  eq "$HTTP_STATUS" 200 "status do inspect"
  eq "$(jget "$WORK/resp.body" checksum_ok)" true "checksum_ok"
  eq "$(jget "$WORK/resp.body" file_hash)" "$csv_id" "file_hash"
  step_end "POST /inspect: checksum_ok, file_hash igual ao id"
}

phase_ledger() {
  auth_http GET "/api/v1/usage?events=500"
  local verdict
  verdict=$(ledger_violations)
  local checked=${verdict% *} bad=${verdict#* }
  step_begin
  eq "$HTTP_STATUS" 200 "status de /usage"
  chk "poucos eventos de decompress/analyze/read com saída ($checked)" test "$checked" -ge 5
  eq "$bad" 0 "eventos de decompress, analyze e read com bytes_in ou cpu_ms zerados"
  step_end "ledger: $checked eventos de decompress, analyze e read com bytes_in > 0 e cpu_ms > 0 (leitura só bytes_in)"
}

phase_limits() {
  local port essence_size
  port=$(free_port)
  SMALL_BASE="http://127.0.0.1:$port"
  start_small_engine "$port"
  if ! wait_small_ready; then
    fatal "o contêiner de limites pequenos não ficou pronto em ${READY_TIMEOUT} s"
  fi

  local saved=$BASE
  BASE=$SMALL_BASE

  head -c 2097152 /dev/zero >"$WORK/dois-mib.bin"
  auth_upload "/api/v1/compress?filename=dois-mib.bin&persist=false" "$WORK/dois-mib.bin"
  step_begin
  eq "$HTTP_STATUS" 413 "corpo de 2 MiB com SYNTRA_MAX_BODY_MB=1"
  eq "$(hdr content-type | cut -d';' -f1)" application/json "content-type do 413"
  eq "$(jget "$WORK/resp.body" error.code)" payload_too_large "error.code"
  chk "mensagem do 413 vazia" test -n "$(jget "$WORK/resp.body" error.message)"
  match "$(jget "$WORK/resp.body" error.hint)" 'SYNTRA_MAX_BODY_MB' "hint do 413"
  step_end "corpo acima do limite: 413 em JSON padrão (payload_too_large, mensagem e hint com SYNTRA_MAX_BODY_MB)"

  auth_upload "/api/v1/decompress" "$WORK/dois-mib.bin"
  local decompress_status=$HTTP_STATUS
  auth_upload "/api/v1/analyze" "$WORK/dois-mib.bin"
  step_begin
  eq "$decompress_status" 413 "decompress com corpo grande"
  eq "$HTTP_STATUS" 413 "analyze com corpo grande"
  eq "$(jget "$WORK/resp.body" error.code)" payload_too_large "error.code do analyze"
  step_end "413 em JSON também em /decompress e /analyze"

  auth_upload "/api/v1/compress?filename=um-byte.bin&persist=false" "$WORK/um-byte.bin"
  step_begin
  eq "$HTTP_STATUS" 200 "corpo pequeno com limite de 1 MiB"
  step_end "corpo abaixo do limite continua aceito (200)"

  essence_size=$(fsize "$WORK/vendas.csv.syntra")
  auth_upload "/api/v1/decompress" "$WORK/vendas.csv.syntra"
  step_begin
  chk "a essência do CSV ($essence_size B) deveria caber no corpo de 1 MiB" test "$essence_size" -lt 1000000
  eq "$HTTP_STATUS" 422 "essência de 5 MB descomprimidos com teto de 1 MiB"
  eq "$(jget "$WORK/resp.body" error.code)" decompressed_too_large "error.code"
  match "$(jget "$WORK/resp.body" error.hint)" 'SYNTRA_MAX_DECOMPRESSED_MB' "hint do teto de descompressão"
  step_end "teto de descompressão (padrão = SYNTRA_MAX_BODY_MB = 1): envelope legítimo de 5 MB devolve 422 decompressed_too_large"

  BASE=$saved
  docker rm -f "$SMALL_NAME" >/dev/null 2>&1 || true
  docker volume rm -f "$SMALL_VOLUME" >/dev/null 2>&1 || true
}

phase_tenants() {
  local csv_id=${OBJ_ID[vendas.csv]}
  local nd_id=${OBJ_ID[eventos.ndjson]}
  local nd_size
  nd_size=$(fsize "$WORK/eventos.ndjson")

  http POST "/api/v1/admin/tenants" -H "x-api-key: $ADMIN_KEY" -H 'content-type: application/json' \
    --data '{"id":"smoke-acme","name":"ACME Smoke","plan":"savings_share","price":{"share_pct":30,"reference_gb_month_cents":12}}'
  step_begin
  eq "$HTTP_STATUS" 200 "criação do tenant"
  eq "$(jget "$WORK/resp.body" id)" smoke-acme "id do tenant"
  eq "$(jget "$WORK/resp.body" plan)" savings_share "plano"
  step_end "POST /admin/tenants: tenant smoke-acme criado"

  http POST "/api/v1/admin/tenants/smoke-acme/keys" -H "x-api-key: $ADMIN_KEY" -H 'content-type: application/json' \
    --data '{"name":"producao","scopes":["compress","read"],"expires_in_days":30}'
  local tenant_key key_id
  tenant_key=$(jget "$WORK/resp.body" key)
  key_id=$(jget "$WORK/resp.body" key_id)
  step_begin
  eq "$HTTP_STATUS" 200 "emissão da chave"
  match "$tenant_key" '^syn_[0-9a-f]{12}_[0-9a-f]{64}$' "formato da chave"
  chk "expires_at_ms ausente" test "$(jget "$WORK/resp.body" expires_at_ms)" != null
  step_end "POST /admin/tenants/:id/keys: chave syn_<id>_<segredo> emitida, com expiração"

  http GET "/api/v1/admin/tenants/smoke-acme/keys" -H "x-api-key: $ADMIN_KEY"
  step_begin
  eq "$HTTP_STATUS" 200 "listagem de chaves"
  chk "listagem de chaves devolveu o segredo" bash -c '! grep -qF -- "$1" "$2"' _ "$tenant_key" "$WORK/resp.body"
  step_end "GET /admin/tenants/:id/keys: só metadados, o segredo não reaparece"

  upload POST "/api/v1/compress?filename=eventos.ndjson&effort=balanced&response=json" "$WORK/eventos.ndjson" \
    -H "x-api-key: $tenant_key" -H 'idempotency-key: smoke-idem-1'
  local first_id
  first_id=$(jget "$WORK/resp.body" id)
  step_begin
  eq "$HTTP_STATUS" 200 "compress com chave do tenant"
  eq "$first_id" "$nd_id" "id (mesmo conteúdo do tenant default)"
  eq "$(jget "$WORK/resp.body" deduplicated)" false "deduplicated entre tenants não deve ser revelado"
  eq "$(jget "$WORK/resp.body" replayed)" false "replayed no primeiro envio"
  eq "$(jget "$WORK/resp.body" references)" 1 "references do novo tenant"
  step_end "compress do tenant com conteúdo já existente: deduplicated=false (sem vazar dedup entre tenants)"

  upload POST "/api/v1/compress?filename=eventos.ndjson&effort=balanced&response=json" "$WORK/eventos.ndjson" \
    -H "x-api-key: $tenant_key" -H 'idempotency-key: smoke-idem-1'
  step_begin
  eq "$HTTP_STATUS" 200 "reenvio com a mesma Idempotency-Key"
  eq "$(jget "$WORK/resp.body" replayed)" true "replayed"
  eq "$(jget "$WORK/resp.body" id)" "$nd_id" "id"
  step_end "mesma Idempotency-Key: replayed=true, sem reprocessar"

  http GET "/api/v1/usage?events=5" -H "x-api-key: $tenant_key"
  step_begin
  eq "$HTTP_STATUS" 200 "status de /usage"
  eq "$(jget "$WORK/resp.body" tenant_id)" smoke-acme "tenant_id"
  eq "$(jget "$WORK/resp.body" totals.events)" 1 "totals.events (replay não gera evento)"
  eq "$(jget "$WORK/resp.body" totals.bytes_in)" "$nd_size" "totals.bytes_in"
  eq "$(jget "$WORK/resp.body" totals.dedup_cross_tenant)" 1 "dedup_cross_tenant"
  eq "$(jget "$WORK/resp.body" totals.dedup_same_tenant)" 0 "dedup_same_tenant"
  eq "$(jget "$WORK/resp.body" quote.plan)" savings_share "quote.plan"
  chk "quote.amount_micro_cents deveria ser > 0" test "$(jget "$WORK/resp.body" quote.amount_micro_cents)" -gt 0
  step_end "GET /usage: 1 evento, bytes_in=$nd_size, dedup entre tenants registrado, cotação savings_share de $(jget "$WORK/resp.body" quote.amount_micro_cents) micro-centavos"

  http GET "/api/v1/objects/$csv_id" -H "x-api-key: $tenant_key"
  local iso_status=$HTTP_STATUS
  http GET "/api/v1/objects" -H "x-api-key: $tenant_key"
  step_begin
  eq "$iso_status" 404 "objeto de outro tenant"
  eq "$(jget "$WORK/resp.body" count)" 1 "objetos visíveis ao tenant"
  step_end "isolamento: objeto do tenant default devolve 404 e a listagem do tenant mostra só o dele"

  http GET "/api/v1/admin/tenants" -H "x-api-key: $tenant_key"
  local admin_status=$HTTP_STATUS
  http DELETE "/api/v1/objects/$nd_id" -H "x-api-key: $tenant_key"
  step_begin
  eq "$admin_status" 403 "rota admin com escopo compress,read"
  eq "$HTTP_STATUS" 403 "DELETE sem escopo delete"
  eq "$(jget "$WORK/resp.body" error.code)" forbidden "error.code"
  step_end "escopos: chave compress,read recebe 403 forbidden em /admin e em DELETE"

  http DELETE "/api/v1/admin/keys/$key_id" -H "x-api-key: $ADMIN_KEY"
  local revoke_status=$HTTP_STATUS
  upload POST "/api/v1/compress?persist=false" "$WORK/um-byte.bin" -H "x-api-key: $tenant_key"
  step_begin
  eq "$revoke_status" 200 "revogação"
  eq "$HTTP_STATUS" 401 "uso da chave revogada"
  step_end "DELETE /admin/keys/:key_id: chave revogada passa a receber 401 na hora"
}

phase_cli() {
  local out="$WORK/cli.out"
  : >"$out"
  local cli_dir="$WORK/cli"
  mkdir -p "$cli_dir"
  cp "$WORK/eventos.ndjson" "$cli_dir/relatório de eventos.ndjson"
  local nd_id=${OBJ_ID[eventos.ndjson]}
  local csv_id=${OBJ_ID[vendas.csv]}
  local rc

  export SYNTRA_URL="$BASE"
  export SYNTRA_API_KEY="$ADMIN_KEY"

  rc=0
  bash "$CLI" health >>"$out" 2>&1 || rc=$?
  step_begin
  eq "$rc" 0 "código de saída"
  chk "health sem 'ready'" grep -Eq 'ready:\s+sim' "$out"
  step_end "CLI health: servidor pronto, código 0"

  rc=0
  bash "$CLI" compress "$cli_dir/relatório de eventos.ndjson" --effort balanced >>"$out" 2>&1 || rc=$?
  step_begin
  eq "$rc" 0 "código de saída"
  chk "container não foi gravado" test -s "$cli_dir/relatório de eventos.ndjson.syntra"
  chk "saída sem 'verificado:  true'" grep -Eq 'verificado:\s+true' "$out"
  step_end "CLI compress (nome com espaço e acento): container gravado, verificado"

  rc=0
  bash "$CLI" decompress "$cli_dir/relatório de eventos.ndjson.syntra" --out "$cli_dir/restaurado.ndjson" >>"$out" 2>&1 || rc=$?
  step_begin
  eq "$rc" 0 "código de saída"
  chk "restaurado difere do original" same_file "$WORK/eventos.ndjson" "$cli_dir/restaurado.ndjson"
  step_end "CLI decompress --out: bytes idênticos (cmp)"

  rc=0
  bash "$CLI" decompress "$cli_dir/relatório de eventos.ndjson.syntra" --out "$cli_dir/restaurado.ndjson" >>"$out" 2>&1 || rc=$?
  step_begin
  eq "$rc" 2 "código de saída ao recusar sobrescrita"
  step_end "CLI recusa sobrescrever arquivo existente sem --force (código 2)"

  rc=0
  bash "$CLI" get "$csv_id" --out "$cli_dir/vendas.get.csv" >>"$out" 2>&1 || rc=$?
  step_begin
  eq "$rc" 0 "código de saída"
  chk "get difere do original" same_file "$WORK/vendas.csv" "$cli_dir/vendas.get.csv"
  step_end "CLI get ID --out: bytes idênticos (cmp)"

  rc=0
  bash "$CLI" analyze "$WORK/vendas.csv" --effort balanced >"$cli_dir/analyze.txt" 2>&1 || rc=$?
  cat "$cli_dir/analyze.txt" >>"$out"
  step_begin
  eq "$rc" 0 "código de saída"
  chk "analyze sem 'recomendado:'" grep -Eq 'recomendado:\s+\S+' "$cli_dir/analyze.txt"
  step_end "CLI analyze: tabela de candidatos e plano recomendado"

  rc=0
  bash "$CLI" inspect "$WORK/vendas.csv.syntra" >"$cli_dir/inspect.txt" 2>&1 || rc=$?
  cat "$cli_dir/inspect.txt" >>"$out"
  step_begin
  eq "$rc" 0 "código de saída"
  chk "inspect sem checksum ok" grep -Eq 'checksum:\s+ok' "$cli_dir/inspect.txt"
  chk "inspect sem o hash do arquivo" grep -q "$csv_id" "$cli_dir/inspect.txt"
  step_end "CLI inspect: checksum ok e hash do original"

  rc=0
  bash "$CLI" list --search eventos >"$cli_dir/list.txt" 2>&1 || rc=$?
  cat "$cli_dir/list.txt" >>"$out"
  step_begin
  eq "$rc" 0 "código de saída"
  chk "list sem o id do ndjson" grep -q "$nd_id" "$cli_dir/list.txt"
  step_end "CLI list --search: encontra o objeto pelo nome"

  rc=0
  bash "$CLI" stats >"$cli_dir/stats.txt" 2>&1 || rc=$?
  bash "$CLI" usage --events 3 >"$cli_dir/usage.txt" 2>&1 || rc=$((rc + $?))
  bash "$CLI" codecs >"$cli_dir/codecs.txt" 2>&1 || rc=$((rc + $?))
  cat "$cli_dir/stats.txt" "$cli_dir/usage.txt" "$cli_dir/codecs.txt" >>"$out"
  step_begin
  eq "$rc" 0 "soma dos códigos de saída de stats, usage e codecs"
  chk "stats sem 'banco:'" grep -Eq "banco:\s+$DB_KIND" "$cli_dir/stats.txt"
  chk "usage sem 'plano:'" grep -Eq 'plano:\s+savings_share' "$cli_dir/usage.txt"
  chk "codecs sem brotli" grep -q '^brotli' "$cli_dir/codecs.txt"
  step_end "CLI stats, usage e codecs: saída legível, código 0"

  rc=0
  bash "$CLI" get "$(printf 'b%.0s' $(seq 1 64))" >"$cli_dir/err404.txt" 2>&1 || rc=$?
  cat "$cli_dir/err404.txt" >>"$out"
  step_begin
  eq "$rc" 1 "código de saída para erro da API"
  chk "mensagem sem 'not_found'" grep -q 'not_found' "$cli_dir/err404.txt"
  step_end "CLI get de id inexistente: erro legível (404 not_found) e código 1"

  rc=0
  bash "$CLI" get zzz >"$cli_dir/errid.txt" 2>&1 || rc=$?
  cat "$cli_dir/errid.txt" >>"$out"
  step_begin
  eq "$rc" 2 "código de saída para id malformado"
  step_end "CLI get com id malformado: recusado no cliente (código 2)"

  rc=0
  SYNTRA_API_KEY="chave-errada-$RUN_ID" bash "$CLI" list >"$cli_dir/err401.txt" 2>&1 || rc=$?
  cat "$cli_dir/err401.txt" >>"$out"
  step_begin
  eq "$rc" 1 "código de saída para 401"
  chk "mensagem sem 'unauthorized'" grep -q 'unauthorized' "$cli_dir/err401.txt"
  chk "a chave errada apareceu na saída" bash -c '! grep -qF -- "$1" "$2"' _ "chave-errada-$RUN_ID" "$cli_dir/err401.txt"
  step_end "CLI com chave errada: 401 unauthorized legível, código 1, chave não impressa"

  rc=0
  SYNTRA_URL="http://127.0.0.1:1" bash "$CLI" health >"$cli_dir/errnet.txt" 2>&1 || rc=$?
  cat "$cli_dir/errnet.txt" >>"$out"
  step_begin
  eq "$rc" 3 "código de saída para falha de conexão"
  step_end "CLI com serviço inalcançável: código 3"

  rc=0
  bash "$CLI" compress "$WORK/vazio.bin" --out "$cli_dir/vazio.syntra" >>"$out" 2>&1 || rc=$?
  step_begin
  eq "$rc" 0 "compress de arquivo vazio"
  chk "container do arquivo vazio não gravado" test -s "$cli_dir/vazio.syntra"
  step_end "CLI compress de arquivo vazio: container gravado ($(fsize "$cli_dir/vazio.syntra") B)"

  step_begin
  chk "a chave de API apareceu na saída do CLI" bash -c '! grep -qF -- "$1" "$2"' _ "$ADMIN_KEY" "$out"
  step_end "CLI nunca imprimiu a chave de API em nenhuma das saídas acima"

  unset SYNTRA_URL SYNTRA_API_KEY
}

phase_watch() {
  head -n 5000 "$WORK/vendas.csv" >"$WORK/watch-smoke.csv"
  docker exec -i "$NAME" sh -c 'cat > /data/watch_in/watch-smoke.csv' <"$WORK/watch-smoke.csv"
  step_begin
  if wait_for_object "watch-smoke" 40; then
    local wid
    wid=$(jget "$WORK/resp.body" items.0.id)
    WATCH_ID=$wid
    HTTP_OUT="$WORK/watch.content" auth_http GET "/api/v1/objects/$wid/content"
    eq "$HTTP_STATUS" 200 "status de /content do objeto ingerido"
    chk "conteúdo ingerido difere do arquivo colocado no diretório" same_file "$WORK/watch-smoke.csv" "$WORK/watch.content"
  else
    STEP_ERRS+=("o arquivo colocado em /data/watch_in não apareceu em /objects em 40 s")
  fi
  step_end "ingestão por diretório: arquivo em /data/watch_in vira objeto do tenant default, bytes idênticos"
}

phase_delete() {
  local id=${OBJ_ID[um-byte.bin]}

  auth_http DELETE "/api/v1/objects/$id"
  step_begin
  eq "$HTTP_STATUS" 200 "DELETE sem purge"
  eq "$(jget "$WORK/resp.body" removed)" false "removed"
  eq "$(jget "$WORK/resp.body" remaining_references)" 1 "remaining_references"
  step_end "DELETE /objects/:id com 2 referências: solta uma, o blob fica (remaining_references=1)"

  auth_http DELETE "/api/v1/objects/$id?purge=true"
  step_begin
  eq "$HTTP_STATUS" 200 "DELETE com purge"
  eq "$(jget "$WORK/resp.body" removed)" true "removed"
  step_end "DELETE /objects/:id?purge=true: blob removido"

  auth_http GET "/api/v1/objects/$id"
  step_begin
  eq "$HTTP_STATUS" 404 "objeto removido"
  step_end "objeto removido devolve 404"
}

phase_restart() {
  local csv_id=${OBJ_ID[vendas.csv]}
  local um_id=${OBJ_ID[um-byte.bin]}
  local start=$SECONDS

  auth_http GET "/api/v1/objects/$WATCH_ID"
  local refs_before
  refs_before=$(jget "$WORK/resp.body" references)
  auth_http GET "/api/v1/usage?days=1"
  COMPRESS_EVENTS_BEFORE=$(usage_compress_events)

  docker restart -t 30 "$NAME" >/dev/null
  step_begin
  if wait_ready; then
    HTTP_OUT="$WORK/vendas.after-restart" auth_http GET "/api/v1/objects/$csv_id/content"
    eq "$HTTP_STATUS" 200 "status de /content após reinício"
    chk "conteúdo após reinício difere do original" same_file "$WORK/vendas.csv" "$WORK/vendas.after-restart"
    auth_http GET "/api/v1/objects/$um_id"
    eq "$HTTP_STATUS" 404 "objeto removido continua removido após reinício"
    auth_http GET "/api/v1/objects?limit=100"
    eq "$(jget "$WORK/resp.body" count)" 5 "objetos listados após reinício (5 enviados, 1 removido, 1 ingerido por diretório)"
  else
    STEP_ERRS+=("o serviço não voltou a responder /ready após docker restart")
  fi
  step_end "reinício do contêiner ($((SECONDS - start)) s): vault, índice e banco persistidos no volume; conteúdo idêntico, remoção preservada"

  sleep 8
  auth_http GET "/api/v1/objects/$WATCH_ID"
  local refs_after
  refs_after=$(jget "$WORK/resp.body" references)
  auth_http GET "/api/v1/usage?days=1"
  local events_after
  events_after=$(usage_compress_events)
  step_begin
  eq "$refs_before" 1 "referências do objeto ingerido por diretório antes do reinício"
  eq "$refs_after" 1 "referências do objeto ingerido por diretório depois do reinício e de uma varredura"
  eq "$events_after" "$COMPRESS_EVENTS_BEFORE" "eventos compress no ledger do tenant default depois do reinício"
  step_end "ingestão por diretório: o arquivo inalterado em /data/watch_in não foi reingerido após o reinício (1 referência, sem novo evento compress)"
}

phase_shutdown() {
  docker stop -t 30 "$NAME" >/dev/null
  local exit_code logs
  exit_code=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo "?")
  logs=$(docker logs "$NAME" 2>&1 || true)
  local sigterms closed
  sigterms=$(grep -c 'SIGTERM recebido' <<<"$logs" || true)
  closed=$(grep -c 'motor encerrado' <<<"$logs" || true)
  step_begin
  eq "$exit_code" 0 "código de saída do contêiner após SIGTERM"
  eq "$sigterms" 2 "ocorrências de 'SIGTERM recebido' no log (reinício + parada)"
  eq "$closed" 2 "ocorrências de 'motor encerrado' no log (reinício + parada)"
  step_end "encerramento gracioso: SIGTERM tratado nas duas paradas, código 0, log 'motor encerrado'"
}

print_table() {
  printf '\n%-18s %9s %9s %7s  %-24s %9s  %-16s %s\n' ARQUIVO TAMANHO ESSENCIA 'ECON %' PLANO CONTAINER CLASSE OK
  local row name size essence savings plan container ok klass
  for row in "${ROWS[@]}"; do
    IFS='|' read -r name size essence savings plan container ok klass <<<"$row"
    printf '%-18s %9s %9s %7s  %-24s %9s  %-16s %s\n' "$name" "$size" "$essence" "$savings" "$plan" "$container" "$klass" "$ok"
  done
  printf '\nTAMANHO, ESSENCIA e CONTAINER em bytes (ESSENCIA = payload; CONTAINER = arquivo .syntra completo).\n'
}

main() {
  preflight
  printf 'Syntra Engine: teste de fumaça (banco: %s, imagem: %s)\n\n' "$DB_KIND" "$IMAGE"

  build_image

  PORT=$(free_port)
  BASE="http://127.0.0.1:$PORT"
  ADMIN_KEY="smoke-$(random_hex 24)"
  docker rm -f "$NAME" >/dev/null 2>&1 || true

  local db_url
  if [ "$DB_KIND" = "postgres" ]; then
    local pg_pass
    pg_pass=$(random_hex 16)
    start_postgres "$pg_pass"
    db_url="postgres://syntra:$pg_pass@$PG_NAME:5432/syntra_smoke"
  else
    db_url="sqlite:/data/smoke.db"
  fi

  local start=$SECONDS
  start_engine "$db_url"
  if wait_ready; then
    pass "serviço no ar em 127.0.0.1:$PORT, /ready respondeu 200 em $((SECONDS - start)) s"
  else
    fatal "o serviço não ficou pronto em ${READY_TIMEOUT} s (log do contêiner abaixo)"
  fi

  make_data
  printf '\n'

  local f
  for f in vendas.csv eventos.ndjson aleatorio.bin vazio.bin um-byte.bin; do
    exercise_file "$f"
  done

  phase_api
  phase_errors
  phase_ledger
  phase_limits
  phase_tenants
  phase_cli
  phase_watch
  phase_delete
  phase_restart
  phase_shutdown

  print_table
  printf '\nRESULTADO: %d PASS, %d FAIL, %d s no total\n' "$PASSED" "$FAILED" "$((SECONDS - STARTED))"
  [ "$FAILED" -eq 0 ]
}

main "$@"
