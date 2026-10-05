#!/usr/bin/env python3
"""A minimal SMTP sink for the parity oracle: accepts every message (and any AUTH) and
appends it, raw, to <outdir>/<port>.mbox as one JSON line per message.

    smtp_sink.py <port> <outdir>

Standard library only (smtpd/asyncore are gone from Python 3.12).
"""
import json
import os
import socketserver
import sys


class Handler(socketserver.StreamRequestHandler):
    def reply(self, line):
        self.wfile.write((line + "\r\n").encode())
        self.wfile.flush()

    def handle(self):
        self.reply("220 parity-sink ESMTP")
        mail_from, rcpt = None, []
        while True:
            raw = self.rfile.readline()
            if not raw:
                return
            line = raw.decode("utf-8", "replace").rstrip("\r\n")
            cmd = line[:4].upper()
            if cmd in ("EHLO", "HELO"):
                self.wfile.write(b"250-parity-sink\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n")
                self.wfile.flush()
            elif cmd == "AUTH":
                parts = line.split()
                if len(parts) == 2 and parts[1].upper() == "LOGIN":
                    self.reply("334 VXNlcm5hbWU6")
                    self.rfile.readline()
                    self.reply("334 UGFzc3dvcmQ6")
                    self.rfile.readline()
                elif len(parts) == 2:
                    self.reply("334 ")
                    self.rfile.readline()
                self.reply("235 2.7.0 Authentication successful")
            elif cmd == "MAIL":
                mail_from, rcpt = line[10:].strip(), []
                self.reply("250 OK")
            elif cmd == "RCPT":
                rcpt.append(line[8:].strip())
                self.reply("250 OK")
            elif cmd == "DATA":
                self.reply("354 End data with <CR><LF>.<CR><LF>")
                body = []
                while True:
                    data = self.rfile.readline()
                    if not data or data in (b".\r\n", b".\n"):
                        break
                    if data.startswith(b".."):
                        data = data[1:]
                    body.append(data)
                record = {"from": mail_from, "to": rcpt, "data": b"".join(body).decode("utf-8", "replace")}
                with open(self.server.outfile, "a", encoding="utf-8") as f:
                    f.write(json.dumps(record) + "\n")
                self.reply("250 OK queued")
            elif cmd == "RSET":
                mail_from, rcpt = None, []
                self.reply("250 OK")
            elif cmd == "QUIT":
                self.reply("221 Bye")
                return
            else:
                self.reply("250 OK")


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


if __name__ == "__main__":
    port, outdir = int(sys.argv[1]), sys.argv[2]
    os.makedirs(outdir, exist_ok=True)
    server = Server(("127.0.0.1", port), Handler)
    server.outfile = os.path.join(outdir, f"{port}.mbox")
    server.serve_forever()
