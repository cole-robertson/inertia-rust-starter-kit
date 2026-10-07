import { type Page, expect } from "@playwright/test"

// Reading mail the app sent during a spec, from e2e/mail-sink.ts.

const sink = `http://127.0.0.1:${process.env.MAIL_SINK_HTTP_PORT ?? 2526}`

/**
 * The last message sent to `email` with `subject`, decoded: the headers unfolded and their
 * RFC 2047 encoded words decoded (so `subject` may be non-ASCII), then quoted-printable
 * (headers and body). Polls: the app may still be handing the message to SMTP when the page has already
 * moved on.
 */
export async function lastMailTo(
  page: Page,
  email: string,
  subject: string,
): Promise<string> {
  let mail = ""
  await expect
    .poll(
      async () => {
        const res = await page.request.get(
          `${sink}/last?to=${encodeURIComponent(email)}`,
        )
        mail = res.ok() ? decode(await res.text()) : ""
        return mail.split(/\r?\n/).includes(`Subject: ${subject}`)
      },
      { message: `a mail to ${email} with subject "${subject}"` },
    )
    .toBe(true)
  return mail
}

/** The path and query of the first link in `mail` to `path` on this app. */
export function linkIn(mail: string, path: string): string {
  const url = new RegExp(`https?://[^\\s"<>]+?(${path}\\?[^\\s"<>]+)`).exec(
    mail,
  )
  expect(url, `the mail has a ${path} link`).not.toBeNull()
  return url![1].replaceAll("&amp;", "&")
}

// Undo what lettre does to a message: unfold the headers (a long one continues on lines that
// start with whitespace) and decode their RFC 2047 encoded words (a non-ASCII header, say a
// subject with "·" or an accent, arrives as `=?utf-8?b?<base64>?=`, split into several words
// when long; the whitespace between two adjacent words isn't text), then quoted-printable soft
// breaks and =XX escapes.
function decode(raw: string) {
  const end = raw.search(/\r?\n\r?\n/)
  const headers = (end < 0 ? raw : raw.slice(0, end))
    .replace(/\r?\n([ \t])/g, "$1")
    .replace(/\?=[ \t]+=\?/g, "?==?")
    .replace(/(?:=\?utf-8\?b\?[A-Za-z0-9+/=]*\?=)+/gi, (run) =>
      Buffer.concat(
        [...run.matchAll(/=\?utf-8\?b\?([A-Za-z0-9+/=]*)\?=/gi)].map((word) =>
          Buffer.from(word[1], "base64"),
        ),
      ).toString("utf8"),
    )
  return (headers + (end < 0 ? "" : raw.slice(end)))
    .replace(/=\r?\n/g, "")
    .replace(/=([0-9A-F]{2})/g, (_, hex: string) =>
      String.fromCharCode(parseInt(hex, 16)),
    )
}
