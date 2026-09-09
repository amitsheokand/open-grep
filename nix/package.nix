# Build one-grep with nixpkgs rustPlatform.
# OSS-safe: no private hostnames, product names, or user paths.
{
  lib,
  rustPlatform,
  pkg-config,
  openssl,
  onnxruntime,
}:

rustPlatform.buildRustPackage {
  pname = "one-grep";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: _type:
      let
        name = baseNameOf path;
      in
      name != "target"
      && name != ".git"
      && name != ".one-grep"
      && name != ".DS_Store"
      && name != "build-agent-onegrep.log"
      && !(lib.hasSuffix ".md" name && lib.hasPrefix "RECEIPT-" name)
      && !(lib.hasSuffix ".md" name && lib.hasPrefix "PACKET-" name);
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    openssl
    onnxruntime
  ];

  # Prefer nixpkgs onnxruntime over ort-sys network download (sandbox-safe).
  # Darwin Security/SystemConfiguration come from the stdenv apple-sdk
  # (do not reference legacy darwin.apple_sdk.frameworks stubs).
  ORT_STRATEGY = "system";
  ORT_LIB_LOCATION = "${onnxruntime}/lib";

  # Package check deferred; use `cargo test` / host validation instead.
  doCheck = false;

  meta = with lib; {
    description = "Local-first hybrid workspace search (BM25 + ONNX embeddings + MCP)";
    license = licenses.asl20;
    mainProgram = "one-grep";
    platforms = platforms.unix;
  };
}
