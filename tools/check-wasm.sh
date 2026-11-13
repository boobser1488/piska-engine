#!/usr/bin/env bash
# Проверить, что движок и все игры из games/ собираются для браузера
# (wasm32-unknown-unknown): `cargo check` всего дерева зависимостей.
# Запуск из корня рабочего пространства: tools/check-wasm.sh
set -euo pipefail
cd "$(dirname "$0")/.."

target=wasm32-unknown-unknown
if command -v rustup >/dev/null && ! rustup target list --installed | grep -qx "$target"; then
    echo "the Rust target $target is missing: rustup target add $target" >&2
    exit 1
fi

# Отладочная информация в wasm не нужна и только раздувает каталог target.
export CARGO_PROFILE_DEV_DEBUG=0

packages=(-p engine)
for manifest in games/*/Cargo.toml; do
    [ -f "$manifest" ] || continue
    name=$(sed -n 's/^name *= *"\(.*\)"/\1/p' "$manifest" | head -n 1)
    packages+=(-p "$name")
done

echo "cargo check --target $target ${packages[*]} --no-default-features"
cargo check --target "$target" "${packages[@]}" --no-default-features
echo "OK: everything builds for $target"
