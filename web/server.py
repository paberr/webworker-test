from http.server import HTTPServer, SimpleHTTPRequestHandler

class CustomHandler(SimpleHTTPRequestHandler):
    def end_headers(self):
        # Add COEP and COOP headers
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        # Call the superclass method to finalize headers
        super().end_headers()

if __name__ == "__main__":
    PORT = 8000  # You can change this port if needed
    server = HTTPServer(("localhost", PORT), CustomHandler)
    print(f"Serving on http://localhost:{PORT}")
    server.serve_forever()
