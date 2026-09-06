# Prebuilt-binary Nix derivation for JII (static musl — no compile, no autoPatchelf).
#
# Use it ad hoc:
#   nix-build packaging/nix/jii.nix         # result/bin/jii
# or install imperatively:
#   nix-env -f packaging/nix/jii.nix -i
# or reference it from a flake / overlay as a package. On each release bump `version`
# and refresh both SRI hashes (`nix hash convert --to sri --hash-algo sha256 <hex>`, or
# `nix-prefetch-url <tarball-url>`).
{
  lib ? (import <nixpkgs> { }).lib,
  stdenvNoCC ? (import <nixpkgs> { }).stdenvNoCC,
  fetchurl ? (import <nixpkgs> { }).fetchurl,
  installShellFiles ? (import <nixpkgs> { }).installShellFiles,
}:

let
  version = "0.1.21-beta";
  tag = "v${version}";
  base = "https://github.com/0nigiris/JII/releases/download/${tag}";

  sources = {
    "x86_64-linux" = {
      arch = "x86_64";
      hash = "sha256-yvcqlQfe6kOvtZJtEs3s5RIlgG8sDg0pv9/OCBYsdyU=";
    };
    "aarch64-linux" = {
      arch = "aarch64";
      hash = "sha256-7utvQVnWMbGjcsroXx2oLWZzL6F4b5oUCRja211MbN8=";
    };
  };

  src' = sources.${stdenvNoCC.hostPlatform.system} or (throw
    "jii: no prebuilt binary for ${stdenvNoCC.hostPlatform.system}");
in
stdenvNoCC.mkDerivation {
  pname = "jii";
  inherit version;

  src = fetchurl {
    url = "${base}/jii-${tag}-${src'.arch}-linux.tar.gz";
    inherit (src') hash;
  };

  sourceRoot = "jii-${tag}-${src'.arch}-linux";

  nativeBuildInputs = [ installShellFiles ];

  # Static musl binary: no dynamic loader to patch.
  dontPatchELF = true;
  dontStrip = true;

  installPhase = ''
    runHook preInstall
    install -Dm755 jii "$out/bin/jii"
    install -Dm644 jii.1 "$out/share/man/man1/jii.1"
    installShellCompletion \
      --bash completions/jii.bash \
      --zsh completions/_jii \
      --fish completions/jii.fish
    install -Dm644 LICENSE "$out/share/licenses/jii/LICENSE"
    runHook postInstall
  '';

  meta = with lib; {
    description = "Smart universal package installer for Linux";
    homepage = "https://github.com/0nigiris/JII";
    license = licenses.gpl3Plus;
    platforms = [ "x86_64-linux" "aarch64-linux" ];
    mainProgram = "jii";
  };
}
