// A minimal SMTP sink for the Playwright run: the e2e servers send mail here instead of
// recording it in memory (bin/e2e-server sets MAILER_STUB=false, MAILER_PORT=2525), and specs
// read it back over HTTP (e2e/mail.ts wraps this):
//
//   GET http://127.0.0.1:2526/last?to=<email>   the raw message last sent to <email>, or 404
//
// Plain SMTP, no auth or TLS, in memory, no dependencies: just enough for lettre's client.
// Ports: MAIL_SINK_SMTP_PORT / MAIL_SINK_HTTP_PORT. Run with
// `node --experimental-strip-types e2e/mail-sink.ts` (playwright.config.ts starts it).
import { createServer as createHttp } from "node:http"
import { createServer as createTcp } from "node:net"

const smtpPort = Number(process.env.MAIL_SINK_SMTP_PORT ?? 2525)
const httpPort = Number(process.env.MAIL_SINK_HTTP_PORT ?? 2526)

const messages: { to: string[]; data: string }[] = []

createTcp((socket) => {
  socket.setEncoding("utf8")
  let buffer = ""
  let inData = false
  let data = ""
  let to: string[] = []
  const reply = (line: string) => socket.write(`${line}\r\n`)
  reply("220 mail-sink ready")

  socket.on("data", (chunk: string) => {
    buffer += chunk
    let end: number
    while ((end = buffer.indexOf("\r\n")) >= 0) {
      const line = buffer.slice(0, end)
      buffer = buffer.slice(end + 2)
      if (inData) {
        if (line === ".") {
          inData = false
          messages.push({ to, data })
          to = []
          data = ""
          reply("250 OK")
        } else {
          data += (line.startsWith("..") ? line.slice(1) : line) + "\r\n"
        }
        continue
      }
      const verb = line.slice(0, 4).toUpperCase()
      if (verb === "EHLO" || verb === "HELO") reply("250 mail-sink")
      else if (verb === "MAIL") reply("250 OK")
      else if (verb === "RCPT") {
        const address = /<([^>]*)>/.exec(line)?.[1]
        if (address) to.push(address.toLowerCase())
        reply("250 OK")
      } else if (verb === "DATA") {
        inData = true
        reply("354 End data with <CR><LF>.<CR><LF>")
      } else if (verb === "QUIT") {
        reply("221 Bye")
        socket.end()
      } else reply("250 OK") // RSET, NOOP
    }
  })
  socket.on("error", () => socket.destroy())
}).listen(smtpPort, "127.0.0.1")

createHttp((req, res) => {
  const url = new URL(req.url ?? "/", "http://localhost")
  if (url.pathname === "/up") {
    res.end("ok")
    return
  }
  const to = url.searchParams.get("to")?.toLowerCase()
  const message =
    url.pathname === "/last" && to
      ? messages.findLast((m) => m.to.includes(to))
      : undefined
  if (!message) {
    res.statusCode = 404
    res.end("no mail")
    return
  }
  res.setHeader("content-type", "text/plain; charset=utf-8")
  res.end(message.data)
}).listen(httpPort, "127.0.0.1")
