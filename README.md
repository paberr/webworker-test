# webworker-test
This is a prototype doing BLS public key uncompression in a webworker.

To run the test:
1. Run `wasm-pack build --target web`
2. Copy wasm `cp -r pkg web`
3. Start a webserver `cd web; python3 -m http.server 8000`
4. Open `localhost:8000`
