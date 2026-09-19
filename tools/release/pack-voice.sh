#!/usr/bin/env bash
# 把一档语音识别模型发成一个不可变的语音 Release（voice-vN，预发布），并更新 tools/release/voice.lock。
# 终端用户在设置里按这份清单下载（crates/qingjian-voice 的 fetch）。
#
# 只挑 int8 的 encoder / decoder 加 tokens：上游整包同时带 fp32 与 int8（SenseVoice 那个 999 MB 里
# 894 MB 是 fp32），我们只要 int8，重打包后小得多。**不打成一个压缩包** —— 每个文件一个独立资产，
# 运行时逐个校验 SHA256，就不需要任何解压依赖。
#
#   tools/release/pack-voice.sh --tier base --dir data/voice/sherpa-onnx-whisper-base \
#       --label 标准 --note "中英混说，体积小"
#   tools/release/pack-voice.sh --tier base --dir ... --pack    # 只算 sha 与体积，不上传、不写锁
#
# 换模型（改了文件内容）要发新的标签号：资产不可变，同名覆盖会让已经下过的人生效不了。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/target/release-voice"
LOCK="$ROOT/tools/release/voice.lock"
cd "$ROOT"

MODE=upload
TIER=""
DIR=""
LABEL=""
NOTE=""
TAG="voice-v1"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --tier) TIER="$2"; shift 2 ;;
    --dir) DIR="$2"; shift 2 ;;
    --label) LABEL="$2"; shift 2 ;;
    --note) NOTE="$2"; shift 2 ;;
    --tag) TAG="$2"; shift 2 ;;
    --pack) MODE=pack; shift ;;
    *) echo "未知参数 $1" >&2; exit 1 ;;
  esac
done

[[ -n "$TIER" ]] || { echo "要 --tier <档位名>" >&2; exit 1; }
[[ -n "$DIR" && -d "$DIR" ]] || { echo "--dir 指向解压好的模型目录" >&2; exit 1; }
[[ -n "$LABEL" ]] || { echo "要 --label <界面上显示的名字>" >&2; exit 1; }
[[ "$TAG" =~ ^voice-v[0-9]+$ ]] || { echo "标签要写成 voice-vN：$TAG" >&2; exit 1; }

sha256() { if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1; else sha256sum "$1" | cut -d' ' -f1; fi; }

# 挑文件：int8 优先，同名同时有 fp32 与 int8 时只取 int8；tokens 按后缀找
# （Whisper 的包里叫 base-tokens.txt，不是 tokens.txt）。
pick() {
  local needle="$1" best=""
  for f in "$DIR"/*"$needle"*.onnx; do
    [[ -f "$f" ]] || continue
    [[ "$f" == *".int8."* ]] && { best="$f"; break; }
    [[ -z "$best" ]] && best="$f"
  done
  [[ -n "$best" ]] && basename "$best"
}
ENCODER="$(pick encoder)"
DECODER="$(pick decoder)"
TOKENS="$(cd "$DIR" && ls ./*tokens.txt 2>/dev/null | head -1 | sed 's|^\./||')"
for f in "$ENCODER" "$DECODER" "$TOKENS"; do
  [[ -n "$f" ]] || { echo "在 $DIR 里没找全 encoder / decoder / tokens（int8 优先）" >&2; exit 1; }
done

rm -rf "$OUT/$TIER" && mkdir -p "$OUT/$TIER"
cp "$DIR/$ENCODER" "$DIR/$DECODER" "$DIR/$TOKENS" "$OUT/$TIER/"
# 许可与署名跟着资产走：Whisper 是 MIT，只要求保留版权与许可声明
[[ -f "$DIR/LICENSE" ]] && cp "$DIR/LICENSE" "$OUT/$TIER/" || true

printf '档位 %s：%s / %s / %s\n' "$TIER" "$ENCODER" "$DECODER" "$TOKENS"
# 体积用 wc -c 数，不解析 ls —— 各平台 ls 的列不一样
for f in "$ENCODER" "$DECODER" "$TOKENS"; do
  printf '  %-34s %10s bytes\n' "$f" "$(wc -c < "$OUT/$TIER/$f" | tr -d ' ')"
done

if [[ "$MODE" == "pack" ]]; then
  echo "（--pack：只算到这里，没上传、没写锁）"
  exit 0
fi

REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
BASE="https://github.com/${REPO}/releases/download/${TAG}"

# 资产不可变：标签已存在、且上面已经有同名文件就拒绝，免得改了内容悄悄覆盖
if gh release view "$TAG" >/dev/null 2>&1; then
  for f in "$ENCODER" "$DECODER" "$TOKENS"; do
    if gh release view "$TAG" --json assets --jq '.assets[].name' | grep -qx "$f"; then
      echo "$TAG 上已经有 $f 了；资产不可变，换模型请发 voice-v$(( ${TAG#voice-v} + 1 ))" >&2
      exit 1
    fi
  done
else
  gh release create "$TAG" --prerelease --target "$(git rev-parse HEAD)" --title "语音模型 $TAG" \
    --notes "本地离线语音识别模型（只含 int8 的 encoder / decoder 与 tokens）。不可变；仓库 tools/release/voice.lock 钉住要用哪一版，终端用户在设置里按它下载。"
fi

gh release upload "$TAG" "$OUT/$TIER"/* --clobber=false

# 更新锁文件：去掉旧的这一节再追加新的，文件头的说明保留
BODY="$(
  printf '[%s]\n' "$TIER"
  printf 'label = "%s"\n' "$LABEL"
  printf 'note = "%s"\n' "$NOTE"
  printf 'files = [\n'
  for f in "$ENCODER" "$DECODER" "$TOKENS"; do
    printf '  { name = "%s", url = "%s/%s", sha256 = "%s", size = %s },\n' \
      "$f" "$BASE" "$f" "$(sha256 "$OUT/$TIER/$f")" "$(wc -c < "$OUT/$TIER/$f" | tr -d ' ')"
  done
  printf ']\n'
)"
awk -v tier="$TIER" '
  $0 == "[" tier "]" { skip = 1; next }
  /^\[/ { skip = 0 }
  !skip { print }
' "$LOCK" > "$LOCK.tmp"
# 去掉尾部空行再追加：每次追加都带一个空行，不清的话每加一档就多一截
awk '{ lines[NR] = $0 } END { last = NR; while (last > 0 && lines[last] == "") last--; for (i = 1; i <= last; i++) print lines[i] }' "$LOCK.tmp" > "$LOCK"
printf '\n%s\n' "$BODY" >> "$LOCK"
rm -f "$LOCK.tmp"

echo "已发 ${TAG}，voice.lock 已更新（记得提交；它编在二进制里，改了要重新编译）"
