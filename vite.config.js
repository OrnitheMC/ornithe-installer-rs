import { defineConfig } from "vite";
import { exec } from "node:child_process";

export default defineConfig({
  server: {
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    }
  },
  plugins: [
    {
      name: "cargo-build",
      buildStart: () => {
        return new Promise((resolve, reject) => {
          exec(
            "cargo build --target=wasm32-unknown-unknown --release --quiet;\
            wasm-bindgen --target web --out-dir public/ target/wasm32-unknown-unknown/release/ornithe-installer-rs.wasm",
            (err, stdout, stderr) => {
              if (err) {
                console.log("Stdout:", stdout);
                console.log("Stderr:", stderr);
                reject(err);
              } else {
                resolve();
              }
            },
          );
        });
      },
    },
  ],
  // cargo build --target wasm32-wasip1 --release
  // wasm-bindgen --target web --out-dir $TRUNK_STAGING_DIR target/wasm32-wasip1/release/ornithe-installer-rs.wasm
});
