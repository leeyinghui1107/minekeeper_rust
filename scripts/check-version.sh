#!/usr/bin/env bash
#
# 版本号一致性校验：`Cargo.toml` 是唯一权威，其余三处必须与它相同。
#
# 这几处是手动对齐的，没有任何机制保证一致 —— 发版时漏改一处就会出现
# 「二进制里是 0.1.0、关于对话框和文档写着别的」这种问题，而且不会有任何报错。
#
#   ./scripts/check-version.sh
#
# 在 CI 上如果是 tag 触发（GITHUB_REF=refs/tags/vX.Y.Z），还会要求 tag 与之一致。
#
# 退出码：0 = 一致；1 = 有不一致。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo_v=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
script_v=$(sed -n 's/^VERSION="\(.*\)"/\1/p' scripts/build-all.sh | head -1)
doc_v=$(sed -n 's/^> 版本：v\([0-9][0-9.]*\).*/\1/p' docs/2026-10-20-rust-port-design.md | head -1)
log_v=$(sed -n 's/^## \[v\([0-9][0-9.]*\)\].*/\1/p' CHANGELOG.md | head -1)

if [ -z "$cargo_v" ]; then
    echo "✗ 从 Cargo.toml 取不到版本号" >&2
    exit 1
fi

fail=0
check() { # 名称 值
    if [ -z "$2" ]; then
        echo "  ✗ $1：取不到版本号"
        fail=1
    elif [ "$2" != "$cargo_v" ]; then
        echo "  ✗ $1：$2 ≠ Cargo.toml 的 $cargo_v"
        fail=1
    else
        echo "  ✓ $1：$2"
    fi
}

echo "权威版本（Cargo.toml）：$cargo_v"
check "scripts/build-all.sh" "$script_v"
check "设计文档" "$doc_v"
check "CHANGELOG 最新条目" "$log_v"

# tag 触发时，tag 本身也必须对得上 —— 否则推出去的发布标签指向的是另一个版本。
if [ -n "${GITHUB_REF:-}" ] && [ "${GITHUB_REF#refs/tags/}" != "$GITHUB_REF" ]; then
    tag="${GITHUB_REF#refs/tags/}"
    if [ "$tag" != "v$cargo_v" ]; then
        echo "  ✗ tag：$tag ≠ v$cargo_v"
        fail=1
    else
        echo "  ✓ tag：$tag"
    fi
fi

if [ "$fail" -ne 0 ]; then
    echo "版本号不一致。以 Cargo.toml 为准，同步 scripts/build-all.sh、设计文档头部与 CHANGELOG。" >&2
    exit 1
fi
