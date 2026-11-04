#!/usr/bin/env bash
set -euo pipefail

SYNTRA_URL="${SYNTRA_URL:-http://127.0.0.1:3002}"
SYNTRA_URL="${SYNTRA_URL%/}"
SYNTRA_API_KEY="${SYNTRA_API_KEY:-}"

EX_API=1
EX_USAGE=2
EX_NET=3

TMP_DIR=""
STATUS=""
BODY=""
HEADERS=""

cleanup() {
  if [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ]; then
    rm -rf "$TMP_DIR"
  fi
}
trap cleanup EXIT

usage() {
  cat <<'EOF'
syntra.sh: cliente de linha de comando do Syntra Engine

Uso:
  syntra.sh <comando> [argumentos]

Comandos:
  compress FILE [--effort fast|balanced|max] [--out FILE] [--json] [--force]
      Comprime FILE e grava o container .syntra (padrão: FILE.syntra).
      Com --json imprime o relatório JSON do servidor e não grava o container
      (o objeto continua arquivado no servidor).
  decompress FILE.syntra [--out FILE] [--force]
      Reconstrói o original a partir de um container. Sem --out, escreve na
      saída padrão (recusa se for um terminal).
  get ID [--out FILE] [--force]
      Baixa o original de um objeto arquivado (ID = 64 dígitos hexadecimais).
  analyze FILE [--effort fast|balanced|max] [--json]
      Compara os planos de compressão sem gravar nada.
  inspect FILE.syntra [--json]
      Lê os metadados de um container e valida o checksum, sem reconstruir.
  list [--search TXT] [--limit N] [--json]
      Lista os objetos do tenant da chave usada.
  stats [--json]
      Estado do motor: configuração, vault, banco e acumulados.
  usage [--days N] [--events N] [--json]
      Consumo e cotação do tenant da chave usada.
  codecs [--json]
      Codecs, transforms e modos de esforço que o servidor oferece.
  health [--json]
      Consulta /health e /ready. Sai com código 1 se o servidor não estiver pronto.

Variáveis de ambiente:
  SYNTRA_URL       endereço do serviço (padrão http://127.0.0.1:3002)
  SYNTRA_API_KEY   chave de API, enviada no cabeçalho x-api-key (nunca impressa)

Códigos de saída:
  0 sucesso   1 erro devolvido pela API   2 uso incorreto   3 falha de conexão
EOF
}

die() {
  printf 'erro: %s\n' "$*" >&2
  exit "$EX_USAGE"
}

need_value() {
  if [ "$#" -lt 2 ] || [ -z "${2:-}" ]; then
    die "a opção $1 exige um valor"
  fi
}

check_effort() {
  case "$1" in
    fast | balanced | max) ;;
    *) die "esforço inválido '$1'; use fast, balanced ou max" ;;
  esac
}

check_id() {
  if ! [[ "$1" =~ ^[0-9a-fA-F]{64}$ ]]; then
    die "ID inválido; esperado o hash BLAKE3 do original, 64 dígitos hexadecimais"
  fi
}

check_number() {
  if ! [[ "$2" =~ ^[0-9]+$ ]]; then
    die "o valor de $1 deve ser um inteiro positivo"
  fi
}

check_readable() {
  if [ ! -f "$1" ]; then
    die "arquivo não encontrado: $1"
  fi
  if [ ! -r "$1" ]; then
    die "arquivo sem permissão de leitura: $1"
  fi
}

check_key() {
  case "$SYNTRA_API_KEY" in
    *$'\n'* | *$'\r'*) die "SYNTRA_API_KEY contém quebra de linha" ;;
  esac
}

urlencode() {
  python3 -I -c 'import sys, urllib.parse; sys.stdout.write(urllib.parse.quote(sys.argv[1], safe=""))' "$1"
}

auth_config() {
  if [ -z "$SYNTRA_API_KEY" ]; then
    return 0
  fi
  local escaped=${SYNTRA_API_KEY//\\/\\\\}
  escaped=${escaped//\"/\\\"}
  printf 'header = "x-api-key: %s"\n' "$escaped"
}

ensure_tmp() {
  if [ -z "$TMP_DIR" ]; then
    TMP_DIR="$(mktemp -d)"
  fi
}

request() {
  local method=$1 path=$2 out=$3
  shift 3
  ensure_tmp
  HEADERS="$TMP_DIR/headers"
  BODY="$out"
  : >"$HEADERS"
  STATUS=$(auth_config | curl -sS --connect-timeout 10 -K - -X "$method" \
    -D "$HEADERS" -o "$out" -w '%{http_code}' "$@" "$SYNTRA_URL$path") || {
    printf 'erro: não foi possível falar com %s\n' "$SYNTRA_URL" >&2
    exit "$EX_NET"
  }
}

header_value() {
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
    }' "$HEADERS"
}

report_api_error() {
  python3 -I - "$STATUS" "$BODY" <<'PY'
import json
import sys

sys.stderr.reconfigure(encoding="utf-8", errors="replace")
status, path = sys.argv[1], sys.argv[2]
raw = b""
try:
    with open(path, "rb") as handle:
        raw = handle.read()
    err = json.loads(raw)["error"]
    print(f"erro {status} {err.get('code', '')}: {err.get('message', '')}", file=sys.stderr)
    if err.get("hint"):
        print(f"dica: {err['hint']}", file=sys.stderr)
    if err.get("request_id"):
        print(f"request_id: {err['request_id']}", file=sys.stderr)
except Exception:
    text = raw[:300].decode("utf-8", "replace").strip()
    print(f"erro {status}" + (f": {text}" if text else ""), file=sys.stderr)
PY
}

require_ok() {
  case "$STATUS" in
    2??) return 0 ;;
  esac
  report_api_error
  exit "$EX_API"
}

render() {
  python3 -I - "$@" <<'PY'
import json
import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

mode = sys.argv[1]
path = sys.argv[2]
extra = sys.argv[3:]

with open(path, "rb") as handle:
    data = json.loads(handle.read())


def human(n):
    n = float(n)
    if n < 1000:
        return f"{int(n)} B"
    for unit in ("KB", "MB", "GB", "TB"):
        n /= 1000.0
        if n < 1000 or unit == "TB":
            return f"{n:.2f} {unit}"


def sized(n):
    n = int(float(n))
    if n < 1000:
        return f"{n} B"
    return f"{human(n)} ({n} B)"


def show(rows):
    width = max(len(label) for label, _ in rows)
    for label, value in rows:
        print(f"{(label + ':').ljust(width + 1)}  {value}")


def table(headers, rows, right=()):
    widths = [len(h) for h in headers]
    for row in rows:
        for i, cell in enumerate(row):
            widths[i] = max(widths[i], len(str(cell)))

    def fmt(row):
        cells = []
        for i, cell in enumerate(row):
            text = str(cell)
            cells.append(text.rjust(widths[i]) if i in right else text.ljust(widths[i]))
        return "  ".join(cells).rstrip()

    print(fmt(headers))
    for row in rows:
        print(fmt(row))


def pct(v):
    return f"{float(v):.2f}%"


def analyze(d):
    rows = [
        ("arquivo", d["original_name"]),
        ("classe", f"{d['content_class']} ({d['mime']})"),
        ("tamanho", f"{sized(d['original_size'])}; medido sobre {human(d['measured_bytes'])}"),
        ("entropia", f"{d['entropy_bits_per_byte']} bits/byte; imprimíveis {float(d['printable_ratio']) * 100:.1f}%"),
    ]
    if d.get("detected_stride"):
        rows.append(("periodicidade", f"{d['detected_stride']} bytes"))
    if d.get("detected_columns"):
        rows.append(("colunas", str(d["detected_columns"])))
    if d.get("dictionary_id"):
        rows.append(("dicionário", d["dictionary_id"]))
    rows.append(("esforço", d["effort"]))
    rows.append(("recomendado", d.get("recommended") or "nenhum"))
    show(rows)
    print()
    cands = d["candidates"]
    limit = 12
    body = []
    for c in cands[:limit]:
        if c.get("error"):
            body.append((c["plan"], "erro", "-", "-", c["error"]))
        else:
            body.append((c["plan"], c["output_size"], f"{float(c['savings_pct']):.2f}", f"{float(c['encode_ms']):.1f}", ""))
    table(["PLANO", "SAÍDA (B)", "ECONOMIA %", "MS", "OBS"], body, right=(1, 2, 3))
    if len(cands) > limit:
        print(f"... mais {len(cands) - limit} candidatos (use --json para a lista completa)")


def inspect(d):
    plan = d["plan"]
    rows = [
        ("nome", d["original_name"]),
        ("mime", d["mime"]),
        ("classe", d["content_class"]),
        ("original", sized(d['original_size'])),
        ("essência", sized(d['essence_size'])),
        ("economia", pct(d["savings_pct"])),
        ("plano", plan["label"]),
        ("motivo", d.get("reason") or "-"),
        ("hash", d["file_hash"]),
        ("verificado", str(d["verified_on_write"]).lower()),
        ("checksum", "ok" if d["checksum_ok"] else "falhou"),
        ("container", f"v{d['container_version']}"),
    ]
    if plan.get("dictionary_id"):
        rows.append(("dicionário", plan["dictionary_id"]))
    names = d.get("known_names") or []
    if len(names) > 1:
        rows.append(("outros nomes", ", ".join(names)))
    metrics = d.get("metrics")
    if metrics:
        rows.append(("candidatos", str(metrics["candidates_tried"])))
        rows.append(("compressão", f"{metrics['processing_time_ms']} ms"))
    show(rows)


def listing(d):
    items = d["items"]
    if not items:
        print("nenhum objeto")
        return
    body = [
        (
            i["original_name"],
            i["original_size"],
            i["essence_size"],
            f"{float(i['savings_pct']):.2f}",
            i["references"],
            i["id"],
        )
        for i in items
    ]
    table(["NOME", "ORIGINAL (B)", "ESSÊNCIA (B)", "ECONOMIA %", "REFS", "ID"], body, right=(1, 2, 3, 4))
    print(f"{d['count']} objeto(s), limite {d['limit']}")


def stats(d):
    cfg = d["config"]
    life = d["lifetime"]
    proc = d["process"]
    show([
        ("motor", f"{d['engine']} {d['version']} (container v{d['container_version']})"),
        ("banco", d["database"]["backend"]),
        ("autenticação", "exigida" if cfg["auth_enabled"] else "aberta"),
        ("esforço padrão", cfg["default_effort"]),
        ("verifica gravação", str(cfg["verify_on_write"]).lower()),
        ("dicionários", "ativos" if cfg["dictionaries_enabled"] else "desativados"),
        ("corpo máximo", f"{cfg['max_body_mb']} MB"),
        ("permits de CPU", f"{cfg['available_permits']}/{cfg['compress_permits']} livres"),
        ("vault", f"{d['vault']['objects_indexed']} objetos em {d['vault']['path']}"),
        ("acumulado (banco)", f"{human(life['bytes_in'])} entrada, {human(life['bytes_out'])} saída, economia {pct(life['savings_pct'])}, {life['items_processed']} itens"),
        ("este processo", f"{human(proc['throughput']['bytes_in'])} entrada, {proc['throughput']['items_processed']} itens, {proc['throughput']['verify_failures']} falhas de verificação, ativo há {proc['uptime_seconds']} s"),
    ])


def usage(d):
    t = d["totals"]
    q = d["quote"]
    show([
        ("tenant", d["tenant_id"]),
        ("eventos", str(t["events"])),
        ("entrada", sized(t['bytes_in'])),
        ("saída", sized(t['bytes_out'])),
        ("economizado", f"{human(t['bytes_saved'])} ({pct(t['savings_pct'])})"),
        ("dedup no tenant", str(t["dedup_same_tenant"])),
        ("dedup entre tenants", str(t["dedup_cross_tenant"])),
        ("cpu", f"{t['cpu_ms']} ms"),
        ("plano", q["plan"]),
        ("fórmula", q["formula"]),
        ("valor", f"{q['amount_cents']} centavos ({q['amount_micro_cents']} micro-centavos) {q['currency']}"),
    ])
    ops = t.get("by_operation") or []
    if ops:
        print()
        table(
            ["OPERAÇÃO", "EVENTOS", "ENTRADA (B)", "SAÍDA (B)"],
            [(o["operation"], o["events"], o["bytes_in"], o["bytes_out"]) for o in ops],
            right=(1, 2, 3),
        )
    recent = d.get("recent") or []
    if recent:
        print()
        table(
            ["OPERAÇÃO", "ENTRADA (B)", "SAÍDA (B)", "PLANO", "DEDUP", "OBJETO"],
            [(r["operation"], r["bytes_in"], r["bytes_out"], r["plan"], r["dedup_scope"], r.get("object_id") or "-") for r in recent],
            right=(1, 2),
        )


def codecs(d):
    table(
        ["CODEC", "FAMÍLIA", "NÍVEIS", "PADRÃO", "DICIONÁRIO"],
        [
            (
                c["name"],
                c["family"],
                f"{c['level_range'][0]}..{c['level_range'][1]}",
                c["default_level"],
                "sim" if c["supports_dictionary"] else "não",
            )
            for c in d["codecs"]
        ],
        right=(3,),
    )
    print()
    table(
        ["TRANSFORM", "LARGURAS"],
        [(t["name"], ",".join(str(w) for w in t["widths"])) for t in d["transforms"]],
    )
    print()
    table(
        ["ESFORÇO", "CANDIDATOS", "NOTAS"],
        [(e["name"], e["candidates"], e["notes"]) for e in d["efforts"]],
        right=(1,),
    )
    print()
    print("classes de conteúdo: " + ", ".join(d["content_classes"]))


def health(d):
    with open(extra[0], "rb") as handle:
        ready = json.loads(handle.read())
    checks = ready.get("checks", {})
    show([
        ("serviço", f"{d['engine']} {d['version']} (container v{d['container_version']})"),
        ("health", d["status"]),
        ("ready", f"{'sim' if ready.get('ready') else 'não'} (http {extra[1]})"),
        ("banco", "ok" if checks.get("database") else "falha"),
        ("vault", "ok" if checks.get("vault") else "falha"),
    ])


def raw(d):
    print(json.dumps(d, indent=2, ensure_ascii=False))


handlers = {
    "analyze": analyze,
    "inspect": inspect,
    "list": listing,
    "stats": stats,
    "usage": usage,
    "codecs": codecs,
    "health": health,
    "raw": raw,
}
handlers[mode](data)
PY
}

emit_json() {
  render raw "$1"
}

print_json_or_render() {
  local want_json=$1 mode=$2
  shift 2
  if [ "$want_json" = "1" ]; then
    emit_json "$BODY"
  else
    render "$mode" "$BODY" "$@"
  fi
}

safe_destination() {
  local dest=$1 force=$2
  if [ -e "$dest" ] && [ "$force" != "1" ]; then
    die "o arquivo $dest já existe; use --force para sobrescrever"
  fi
  local dir
  dir="$(dirname "$dest")"
  if [ ! -d "$dir" ]; then
    die "diretório de saída inexistente: $dir"
  fi
}

deliver() {
  local dest=$1
  if [ -n "$dest" ]; then
    mv -f "$BODY" "$dest"
  else
    cat "$BODY"
  fi
}

refuse_binary_to_terminal() {
  if [ -z "$1" ] && [ -t 1 ]; then
    die "a saída é binária; informe --out FILE ou redirecione a saída padrão"
  fi
}

cmd_compress() {
  local file="" effort="" out="" json=0 force=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --effort) need_value "$@"; effort=$2; shift 2 ;;
      --out) need_value "$@"; out=$2; shift 2 ;;
      --json) json=1; shift ;;
      --force) force=1; shift ;;
      -h | --help) usage; exit 0 ;;
      -*) die "opção desconhecida: $1" ;;
      *)
        [ -z "$file" ] || die "informe um único arquivo"
        file=$1
        shift
        ;;
    esac
  done
  [ -n "$file" ] || die "informe o arquivo a comprimir"
  check_readable "$file"
  [ -z "$effort" ] || check_effort "$effort"

  local query="filename=$(urlencode "$(basename "$file")")"
  [ -z "$effort" ] || query="$query&effort=$effort"

  ensure_tmp
  if [ "$json" = "1" ]; then
    request POST "/api/v1/compress?$query&response=json" "$TMP_DIR/body" \
      -T "$file" -H 'content-type: application/octet-stream'
    require_ok
    emit_json "$BODY"
    return 0
  fi

  [ -n "$out" ] || out="$file.syntra"
  safe_destination "$out" "$force"
  request POST "/api/v1/compress?$query" "$TMP_DIR/body" \
    -T "$file" -H 'content-type: application/octet-stream'
  require_ok

  local id original essence savings plan klass verified dedup
  id=$(header_value x-syntra-id)
  original=$(header_value x-syntra-original-size)
  essence=$(header_value x-syntra-essence-size)
  savings=$(header_value x-syntra-savings-pct)
  plan=$(header_value x-syntra-plan)
  klass=$(header_value x-syntra-content-class)
  verified=$(header_value x-syntra-verified)
  dedup=$(header_value x-syntra-deduplicated)
  deliver "$out"

  local container_size
  container_size=$(wc -c <"$out" | tr -d ' ')
  python3 -I - "$(basename "$file")" "$id" "$klass" "$original" "$essence" "$savings" "$plan" "$verified" "$dedup" "$out" "$container_size" <<'PY'
import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
name, ident, klass, original, essence, savings, plan, verified, dedup, out, container = sys.argv[1:]


def human(n):
    n = float(n)
    if n < 1000:
        return f"{int(n)} B"
    for unit in ("KB", "MB", "GB", "TB"):
        n /= 1000.0
        if n < 1000 or unit == "TB":
            return f"{n:.2f} {unit}"


def sized(n):
    n = int(float(n))
    if n < 1000:
        return f"{n} B"
    return f"{human(n)} ({n} B)"


rows = [
    ("arquivo", name),
    ("id", ident),
    ("classe", klass),
    ("original", sized(original)),
    ("essência", sized(essence)),
    ("economia", f"{savings}%"),
    ("plano", plan),
    ("verificado", verified),
    ("deduplicado", dedup),
    ("container", f"{out} ({sized(container)})"),
]
width = max(len(label) for label, _ in rows)
for label, value in rows:
    print(f"{(label + ':').ljust(width + 1)}  {value}")
PY
}

cmd_decompress() {
  local file="" out="" force=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --out) need_value "$@"; out=$2; shift 2 ;;
      --force) force=1; shift ;;
      -h | --help) usage; exit 0 ;;
      -*) die "opção desconhecida: $1" ;;
      *)
        [ -z "$file" ] || die "informe um único container"
        file=$1
        shift
        ;;
    esac
  done
  [ -n "$file" ] || die "informe o container .syntra"
  check_readable "$file"
  refuse_binary_to_terminal "$out"
  [ -z "$out" ] || safe_destination "$out" "$force"

  ensure_tmp
  request POST "/api/v1/decompress" "$TMP_DIR/body" \
    -T "$file" -H 'content-type: application/octet-stream'
  require_ok
  local checked
  checked=$(header_value x-syntra-fidelity-checked)
  deliver "$out"
  if [ -n "$out" ]; then
    printf 'restaurado: %s (%s B, fidelidade conferida: %s)\n' "$out" "$(wc -c <"$out" | tr -d ' ')" "${checked:-desconhecido}" >&2
  fi
}

cmd_get() {
  local id="" out="" force=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --out) need_value "$@"; out=$2; shift 2 ;;
      --force) force=1; shift ;;
      -h | --help) usage; exit 0 ;;
      -*) die "opção desconhecida: $1" ;;
      *)
        [ -z "$id" ] || die "informe um único ID"
        id=$1
        shift
        ;;
    esac
  done
  [ -n "$id" ] || die "informe o ID do objeto"
  check_id "$id"
  refuse_binary_to_terminal "$out"
  [ -z "$out" ] || safe_destination "$out" "$force"

  ensure_tmp
  request GET "/api/v1/objects/$id/content" "$TMP_DIR/body"
  require_ok
  local checked
  checked=$(header_value x-syntra-fidelity-checked)
  deliver "$out"
  if [ -n "$out" ]; then
    printf 'baixado: %s (%s B, fidelidade conferida: %s)\n' "$out" "$(wc -c <"$out" | tr -d ' ')" "${checked:-desconhecido}" >&2
  fi
}

cmd_analyze() {
  local file="" effort="" json=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --effort) need_value "$@"; effort=$2; shift 2 ;;
      --json) json=1; shift ;;
      -h | --help) usage; exit 0 ;;
      -*) die "opção desconhecida: $1" ;;
      *)
        [ -z "$file" ] || die "informe um único arquivo"
        file=$1
        shift
        ;;
    esac
  done
  [ -n "$file" ] || die "informe o arquivo a analisar"
  check_readable "$file"
  [ -z "$effort" ] || check_effort "$effort"

  local query="filename=$(urlencode "$(basename "$file")")"
  [ -z "$effort" ] || query="$query&effort=$effort"

  ensure_tmp
  request POST "/api/v1/analyze?$query" "$TMP_DIR/body" \
    -T "$file" -H 'content-type: application/octet-stream'
  require_ok
  print_json_or_render "$json" analyze
}

cmd_inspect() {
  local file="" json=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --json) json=1; shift ;;
      -h | --help) usage; exit 0 ;;
      -*) die "opção desconhecida: $1" ;;
      *)
        [ -z "$file" ] || die "informe um único container"
        file=$1
        shift
        ;;
    esac
  done
  [ -n "$file" ] || die "informe o container .syntra"
  check_readable "$file"

  ensure_tmp
  request POST "/api/v1/inspect" "$TMP_DIR/body" \
    -T "$file" -H 'content-type: application/octet-stream'
  require_ok
  print_json_or_render "$json" inspect
}

cmd_list() {
  local search="" limit="" json=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --search) need_value "$@"; search=$2; shift 2 ;;
      --limit) need_value "$@"; check_number "$1" "$2"; limit=$2; shift 2 ;;
      --json) json=1; shift ;;
      -h | --help) usage; exit 0 ;;
      *) die "argumento desconhecido: $1" ;;
    esac
  done
  local query=""
  [ -z "$search" ] || query="search=$(urlencode "$search")"
  if [ -n "$limit" ]; then
    query="${query:+$query&}limit=$limit"
  fi

  ensure_tmp
  request GET "/api/v1/objects${query:+?$query}" "$TMP_DIR/body"
  require_ok
  print_json_or_render "$json" list
}

cmd_simple_get() {
  local mode=$1 path=$2
  shift 2
  local json=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --json) json=1; shift ;;
      -h | --help) usage; exit 0 ;;
      *) die "argumento desconhecido: $1" ;;
    esac
  done
  ensure_tmp
  request GET "$path" "$TMP_DIR/body"
  require_ok
  print_json_or_render "$json" "$mode"
}

cmd_usage() {
  local days="" events="" json=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --days) need_value "$@"; check_number "$1" "$2"; days=$2; shift 2 ;;
      --events) need_value "$@"; check_number "$1" "$2"; events=$2; shift 2 ;;
      --json) json=1; shift ;;
      -h | --help) usage; exit 0 ;;
      *) die "argumento desconhecido: $1" ;;
    esac
  done
  local query=""
  [ -z "$days" ] || query="days=$days"
  if [ -n "$events" ]; then
    query="${query:+$query&}events=$events"
  fi

  ensure_tmp
  request GET "/api/v1/usage${query:+?$query}" "$TMP_DIR/body"
  require_ok
  print_json_or_render "$json" usage
}

cmd_health() {
  local json=0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --json) json=1; shift ;;
      -h | --help) usage; exit 0 ;;
      *) die "argumento desconhecido: $1" ;;
    esac
  done
  ensure_tmp
  request GET "/health" "$TMP_DIR/health.json"
  require_ok
  request GET "/ready" "$TMP_DIR/ready.json"
  local ready_status=$STATUS
  if [ "$json" = "1" ]; then
    emit_json "$TMP_DIR/health.json"
    emit_json "$TMP_DIR/ready.json"
  else
    render health "$TMP_DIR/health.json" "$TMP_DIR/ready.json" "$ready_status"
  fi
  case "$ready_status" in
    2??) return 0 ;;
  esac
  return "$EX_API"
}

main() {
  if [ "$#" -eq 0 ]; then
    usage >&2
    exit "$EX_USAGE"
  fi

  local cmd=$1
  shift
  case "$cmd" in
    -h | --help | help)
      usage
      return 0
      ;;
  esac
  check_key

  case "$cmd" in
    compress) cmd_compress "$@" ;;
    decompress) cmd_decompress "$@" ;;
    get) cmd_get "$@" ;;
    analyze) cmd_analyze "$@" ;;
    inspect) cmd_inspect "$@" ;;
    list) cmd_list "$@" ;;
    stats) cmd_simple_get stats /api/v1/stats "$@" ;;
    usage) cmd_usage "$@" ;;
    codecs) cmd_simple_get codecs /api/v1/codecs "$@" ;;
    health) cmd_health "$@" ;;
    *)
      printf 'erro: comando desconhecido: %s\n\n' "$cmd" >&2
      usage >&2
      exit "$EX_USAGE"
      ;;
  esac
}

main "$@"
