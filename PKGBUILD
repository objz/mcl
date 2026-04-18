# Maintainer: objz <me@objz.dev>
pkgname=mcl-git
pkgver=r0.0000000
pkgrel=1
pkgdesc="Minecraft launcher CLI"
arch=('x86_64' 'aarch64')
url="https://github.com/objz/mcl"
license=('GPL-3.0-only')
depends=()
makedepends=('git' 'rust' 'cargo')
provides=('mcl')
conflicts=('mcl')
source=("git+$url.git")
sha256sums=('SKIP')

pkgver() {
  cd "$srcdir/mcl"
  printf "r%s.%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
}

build() {
  cd "$srcdir/mcl"
  cargo build --release --locked
}

check() {
  cd "$srcdir/mcl"
  cargo test --release --locked
}

package() {
  cd "$srcdir/mcl"
  install -Dm755 "target/release/mcl" "$pkgdir/usr/bin/mcl"
  # install -Dm644 README.md "$pkgdir/usr/share/doc/mcl/README.md"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/mcl/LICENSE"
}
