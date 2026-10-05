#!/usr/bin/env python3
"""Drive one running app (the Rails kit or this kit) through every user-visible flow and
write a normalized transcript: status codes, redirect targets, flash, Inertia page objects
(component, url, props, flash, history flags), validation errors, outgoing mail and the
HTML <head>. bench/parity/compare.sh runs it against both apps and diffs the transcripts.

    probe.py <base_url> <mbox_file> <sqlite_db> <out.json>

Normalization removes only what can never match between two independent apps: ids,
timestamps, tokens, asset fingerprints, the Inertia version and the app's host/port.
Everything else, text included, is compared verbatim.
"""
import email
import email.policy
import html
import json
import re
import sqlite3
import sys
import time
import urllib.parse

import http.cookiejar
import urllib.error
import urllib.request

MODERN_UA = (
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) "
    "Chrome/140.0.0.0 Safari/537.36"
)
OLD_UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) "
    "Chrome/100.0.4896.127 Safari/537.36"
)
PASSWORD = "Secret1*3*5*"

base, mbox, db_path, out_path = sys.argv[1:5]
base = base.rstrip("/")
steps = []
version = None


# --------------------------------------------------------------------------- http (stdlib only)


class Response:
    def __init__(self, status, headers, body, url):
        self.status_code = status
        self.headers = headers
        self.content = body
        self.url = url

    @property
    def text(self):
        return self.content.decode("utf-8", "replace")

    def json(self):
        return json.loads(self.content)


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


class Session:
    """requests.Session's subset the probe uses: a cookie jar, default headers, no redirects."""

    def __init__(self):
        self.jar = http.cookiejar.CookieJar()
        self.headers = {}
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(self.jar), _NoRedirect()
        )

    @property
    def cookies(self):
        return {c.name: c.value for c in self.jar}

    def request(self, method, url, json=None, data=None, headers=None, **_):
        return http_request(self.opener, method, url, json, data, {**self.headers, **(headers or {})})

    def get(self, url, **kw):
        return self.request("GET", url, **kw)


def http_request(opener, method, url, json_body=None, data=None, headers=None):
    headers = dict(headers or {})
    body = None
    if json_body is not None:
        body = json.dumps(json_body).encode()
        headers["Content-Type"] = "application/json"
    elif data is not None:
        body = urllib.parse.urlencode(data).encode()
        headers["Content-Type"] = "application/x-www-form-urlencoded"
    req = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        r = opener.open(req, timeout=30)
    except urllib.error.HTTPError as e:
        r = e
    return Response(r.status if hasattr(r, "status") else r.code, r.headers, r.read(), url)


def post(url, json=None, data=None, headers=None, **_):
    return http_request(urllib.request.build_opener(_NoRedirect()), "POST", url, json, data, headers)


# --------------------------------------------------------------------------- helpers


def db(sql, *args):
    con = sqlite3.connect(db_path, timeout=5)
    try:
        cur = con.execute(sql, args)
        con.commit()
        return cur.fetchall()
    finally:
        con.close()


def norm_time(v):
    """Timestamps keep their format, not their value: 2026-09-29T16:15:24.391Z -> 0000-00-00T00:00:00.000Z."""
    return re.sub(r"\d", "0", v) if isinstance(v, str) and re.match(r"^\d{4}-\d\d-\d\dT", v) else v


def norm_props(value, key=None):
    if isinstance(value, dict):
        return {k: norm_props(v, k) for k, v in value.items()}
    if isinstance(value, list):
        return [norm_props(v, key) for v in value]
    if key == "id" and value is not None:
        return "<id>"
    if key == "sid":
        return "<token>"
    if key in ("created_at", "updated_at"):
        return norm_time(value)
    return value


def norm_page(page):
    if page is None:
        return None
    out = {k: v for k, v in page.items() if k not in ("version",)}
    out["props"] = norm_props(page.get("props", {}))
    out["url"] = norm_url(page.get("url"))
    return out


def norm_url(url):
    if url is None:
        return None
    parsed = urllib.parse.urlsplit(url)
    path = parsed.path
    if parsed.query:
        q = urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)
        q = [(k, "<token>" if k == "sid" else v) for k, v in q]
        path += "?" + urllib.parse.urlencode(q)
    return path


class Browser:
    """One cookie jar, like one browser. Speaks Inertia's XHR protocol."""

    def __init__(self, ua=MODERN_UA):
        self.s = Session()
        self.s.headers["User-Agent"] = ua
        self.referer = None

    def xsrf(self):
        token = self.s.cookies.get("XSRF-TOKEN")
        return urllib.parse.unquote(token) if token else None

    def html(self, path, **kw):
        headers = {"Accept": "text/html,application/xhtml+xml"}
        headers.update(kw.pop("headers", {}))
        r = self.s.get(base + path, headers=headers, allow_redirects=False, **kw)
        self.referer = base + path
        return r

    def inertia_headers(self):
        h = {"X-Inertia": "true", "X-Requested-With": "XMLHttpRequest", "Accept": "text/html, application/xhtml+xml"}
        if version is not None:
            h["X-Inertia-Version"] = version
        if self.xsrf():
            h["X-XSRF-TOKEN"] = self.xsrf()
        if self.referer:
            h["Referer"] = self.referer
        return h

    def visit(self, method, path, data=None, headers=None):
        """An Inertia visit: the request, then (like the client) a GET of any redirect."""
        h = self.inertia_headers()
        h.update(headers or {})
        r = self.s.request(method, base + path, json=data, headers=h, allow_redirects=False)
        shown = re.sub(r"^/sessions/[^/?]+", "/sessions/<id>", norm_url(path))
        result = {"request": f"{method} {shown}", "status": r.status_code}
        loc = r.headers.get("Location")
        if loc:
            result["location"] = norm_url(urllib.parse.urljoin(base + path, loc))
        page = None
        hops = 0
        while r.status_code in (301, 302, 303) and hops < 5:
            target = urllib.parse.urljoin(r.url, r.headers["Location"])
            h = self.inertia_headers()
            r = self.s.get(target, headers=h, allow_redirects=False)
            hops += 1
        if r.headers.get("X-Inertia") == "true":
            page = r.json()
            self.referer = base + urllib.parse.urlsplit(page["url"]).path + (
                "?" + urllib.parse.urlsplit(page["url"]).query if urllib.parse.urlsplit(page["url"]).query else ""
            )
            result["final_status"] = r.status_code
            result["page"] = norm_page(page)
        elif hops:
            result["final_status"] = r.status_code
            result["final_location"] = norm_url(r.headers.get("Location")) if r.headers.get("Location") else None
        elif method != "GET" or r.status_code >= 400:
            result["body"] = body_summary(r)
        return result, page


def body_summary(r):
    ctype = r.headers.get("Content-Type", "").split(";")[0]
    text = r.text
    if ctype == "application/json":
        try:
            return {"type": ctype, "json": r.json()}
        except ValueError:
            pass
    if "<html" in text.lower() and "</html>" in text.lower():
        m = re.search(r"<title>(.*?)</title>", text, re.S)
        return {"type": ctype, "title": m.group(1).strip() if m else None, "bytes": len(text.encode())}
    return {"type": ctype, "text": text.strip()[:200]}


def head_of(text):
    """The <head>, one tag per line, with fingerprinted/host-specific values masked."""
    m = re.search(r"<head>(.*?)</head>", text, re.S)
    if not m:
        return None
    head = m.group(1)
    tags = re.findall(r"<(?:title|meta|link|script)\b[^>]*>(?:.*?</(?:title|script)>)?", head, re.S)
    out = []
    for t in tags:
        t = re.sub(r"\s+", " ", t).strip()
        t = re.sub(r' nonce="[^"]*"', "", t)
        t = re.sub(r"/vite/assets/[^\"']+", "/vite/assets/<asset>", t)
        t = re.sub(r'(name="csrf-token" content=")[^"]*', r"\1<token>", t)
        out.append(t)
    return out


HEADER_NAMES = (
    "cache-control", "content-type", "vary", "x-frame-options", "x-content-type-options",
    "referrer-policy", "x-xss-protection", "x-permitted-cross-domain-policies", "x-powered-by",
    "etag",
)


def headers_of(r):
    """The compared response headers; an ETag's value is masked (it hashes the body)."""
    out = {}
    for name in HEADER_NAMES:
        values = r.headers.get_all(name) if hasattr(r.headers, "get_all") else [r.headers.get(name)]
        values = [v for v in (values or []) if v is not None]
        if values:
            out[name] = "<etag>" if name == "etag" else ", ".join(values)
    return out


def page_from_html(text):
    m = re.search(r'<script data-page="app" type="application/json"[^>]*>(.*?)</script>', text, re.S)
    return json.loads(html.unescape(m.group(1))) if m else None


mail_seen = 0


def mails(expect, timeout=10.0):
    """Messages delivered since the last call; waits for `expect` of them."""
    global mail_seen
    deadline = time.time() + timeout
    lines = []
    while time.time() < deadline:
        try:
            with open(mbox, encoding="utf-8") as f:
                lines = f.read().splitlines()[mail_seen:]
        except FileNotFoundError:
            lines = []
        if len(lines) >= expect:
            break
        time.sleep(0.1)
    if expect == 0:
        time.sleep(1.0)
        with open(mbox, encoding="utf-8") as f:
            lines = f.read().splitlines()[mail_seen:]
    mail_seen += len(lines)
    return [parse_mail(json.loads(line)) for line in lines]


def parse_mail(record):
    msg = email.message_from_string(record["data"], policy=email.policy.default)
    parts = {}
    for part in msg.walk():
        if part.get_content_maintype() == "multipart":
            continue
        parts[part.get_content_type()] = part.get_content().replace("\r\n", "\n")
    links = []
    for body in parts.values():
        links += re.findall(r"https?://[^\s\"'<>]+", body)
    link = next((l for l in links if "sid=" in l), None)

    def mask(body):
        return re.sub(r"https?://[^\s\"'<>]+", lambda m: norm_url(html.unescape(m.group(0))), body)

    return {
        "subject": msg["Subject"],
        "from": msg["From"],
        "to": msg["To"],
        "content_types": sorted(parts),
        "bodies": {k: mask(v) for k, v in sorted(parts.items())},
        "_link": html.unescape(link) if link else None,
    }


def step(name, result, **extra):
    entry = {"step": name}
    entry.update(result)
    entry.update(extra)
    steps.append(entry)
    return entry


def public_link(mail):
    link = mail.pop("_link")
    parsed = urllib.parse.urlsplit(link)
    return parsed.path + "?" + parsed.query


# --------------------------------------------------------------------------- flows


def flows():
    global version

    guest = Browser()
    r = guest.html("/")
    first_page = page_from_html(r.text)
    version = first_page["version"] if first_page else None
    step("GET / (html, guest)", {"status": r.status_code, "head": head_of(r.text), "page": norm_page(first_page)})

    for path in ("/sign_in", "/sign_up", "/identity/password_reset/new"):
        r = guest.html(path)
        step(f"GET {path} (html)", {"status": r.status_code, "head": head_of(r.text), "page": norm_page(page_from_html(r.text))})

    for path in ("/dashboard", "/settings/profile", "/settings/password", "/settings/email", "/settings/sessions", "/settings/appearance"):
        res, _ = guest.visit("GET", path)
        step(f"GET {path} (guest)", res)

    r = guest.html("/up")
    step("GET /up", {"status": r.status_code, "body": body_summary(r)})
    r = guest.html("/nope")
    step("GET /nope (404)", {"status": r.status_code, "body": body_summary(r)})
    r = guest.html("/sessions/1")
    step("GET /sessions/1 (no GET route)", {"status": r.status_code, "body": body_summary(r)})
    r = guest.html("/robots.txt")
    step("GET /robots.txt", {"status": r.status_code, "body": body_summary(r)})
    for path in ("/icon.png", "/icon.svg", "/404.html", "/406-unsupported-browser.html"):
        r = guest.html(path)
        step(f"GET {path}", {"status": r.status_code, "bytes": len(r.content)})

    # Response headers of each kind of response.
    for label, path, kw in (
        ("html page", "/sign_in", {}),
        ("inertia page", "/sign_in", {"headers": {"X-Inertia": "true", "X-Inertia-Version": version or ""}}),
        ("redirect", "/dashboard", {}),
        ("404", "/nope", {}),
        ("public file", "/robots.txt", {}),
        ("health", "/up", {}),
        ("health (json)", "/up", {"headers": {"Accept": "application/json"}}),
    ):
        r = guest.html(path, **kw)
        step(f"headers: {label}", {"status": r.status_code, "headers": headers_of(r)})

    r = guest.html("/up", headers={"Accept": "application/json"})
    body = json.loads(r.text) if r.headers.get("Content-Type", "").startswith("application/json") else r.text
    if isinstance(body, dict) and "timestamp" in body:
        body["timestamp"] = re.sub(r"\d", "0", body["timestamp"])
    step("GET /up (json)", {"status": r.status_code, "body": body})
    r = guest.html("/nope", headers={"Accept": "application/json"})
    step("GET /nope (json)", {"status": r.status_code, "body": body_summary(r)})

    for path in ("/sign_in/", "/dashboard/", "/settings/profile/"):
        r = guest.html(path)
        step(f"GET {path} (trailing slash)", {"status": r.status_code, "location": norm_url(r.headers.get("Location"))})

    r = guest.s.request("PATCH", base + "/nope", headers={"Accept": "text/html", "X-XSRF-TOKEN": guest.xsrf() or ""})
    step("PATCH /nope", {"status": r.status_code, "body": body_summary(r)})
    r = guest.s.request("HEAD", base + "/nope")
    step("HEAD /nope", {"status": r.status_code, "bytes": len(r.content)})

    old = Browser(OLD_UA)
    for path in ("/", "/sign_in", "/up"):
        r = old.html(path)
        step(f"GET {path} (old browser)", {"status": r.status_code, "body": body_summary(r)})

    # CSRF: a POST without the token.
    bare = post(base + "/sign_in", json={"email": "x", "password": "y"},
                         headers={"User-Agent": MODERN_UA, "X-Inertia": "true"}, allow_redirects=False)
    step("POST /sign_in without CSRF token (inertia)", {"status": bare.status_code, "body": body_summary(bare)})
    bare = post(base + "/sign_in", data={"email": "x", "password": "y"},
                         headers={"User-Agent": MODERN_UA, "Accept": "text/html"}, allow_redirects=False)
    step("POST /sign_in without CSRF token (html form)", {"status": bare.status_code, "body": body_summary(bare)})

    for ua_label, ua in (
        ("firefox 115", "Mozilla/5.0 (X11; Linux x86_64; rv:109.0) Gecko/20100101 Firefox/115.0"),
        ("firefox 140", "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0"),
        ("safari 16", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Safari/605.1.15"),
        ("safari 17.4", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15"),
        ("ios safari 16", "Mozilla/5.0 (iPhone; CPU iPhone OS 16_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148"),
        ("ie 11", "Mozilla/5.0 (Windows NT 10.0; WOW64; Trident/7.0; rv:11.0) like Gecko"),
        ("opera 100", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/114.0.0.0 Safari/537.36 OPR/100.0.0.0"),
        ("edge 100", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/100.0.4896.127 Safari/537.36 Edg/100.0.1185.44"),
        ("old googlebot", "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html) Chrome/100.0.0.0 Safari/537.36"),
        ("curl", "curl/8.0.1"),
        ("bare Firefox/50", "Firefox/50.0"),
    ):
        r = Browser(ua).html("/sign_in")
        step(f"GET /sign_in ({ua_label})", {"status": r.status_code})

    # Missing params.
    empty = Browser()
    empty.html("/sign_in")
    res, _ = empty.visit("POST", "/sign_in", {})
    step("POST /sign_in no params", {k: v for k, v in res.items() if k != "body"}, body_type=res.get("body", {}).get("type"))

    # ---- sign up
    alice = Browser()
    alice.html("/sign_up")
    res, _ = alice.visit("POST", "/sign_up", {"name": "", "email": "not-an-email", "password": "short", "password_confirmation": "different"})
    step("POST /sign_up invalid", res)
    res, _ = alice.visit("POST", "/sign_up", {"name": "Alice", "email": "alice@example.com", "password": "", "password_confirmation": ""})
    step("POST /sign_up blank password", res)
    longpw = Browser()
    longpw.html("/sign_up")
    res, _ = longpw.visit("POST", "/sign_up", {"name": "Long", "email": "long@example.com", "password": "x" * 73, "password_confirmation": "x" * 73})
    step("POST /sign_up 73-byte password", res)
    res, _ = longpw.visit("POST", "/sign_up", {"name": "Long", "email": "long@example.com", "password": "é" * 37, "password_confirmation": "é" * 37})
    step("POST /sign_up 37 two-byte chars (74 bytes)", res)
    res, _ = alice.visit("POST", "/sign_up", {"name": "Alice", "email": "  Alice@Example.COM ", "password": PASSWORD, "password_confirmation": PASSWORD})
    step("POST /sign_up valid (normalized email)", res, mails=[{k: v for k, v in m.items() if k != "_link"} for m in mails(1)])

    bob = Browser()
    bob.html("/sign_up")
    res, _ = bob.visit("POST", "/sign_up", {"name": "Bob", "email": "alice@example.com", "password": PASSWORD, "password_confirmation": PASSWORD})
    step("POST /sign_up duplicate email", res)
    res, _ = bob.visit("POST", "/sign_up", {"name": "Bob", "email": "bob@example.com", "password": PASSWORD, "password_confirmation": PASSWORD})
    step("POST /sign_up bob", res)
    mails(1)
    # Unverified sign-ups; make alice verified like the Rails fixtures (users(:one) is verified).
    db("UPDATE users SET verified = 1 WHERE email = 'alice@example.com'")

    # ---- signed in
    res, _ = alice.visit("GET", "/sign_in")
    step("GET /sign_in (signed in)", res)
    res, _ = alice.visit("GET", "/sign_up")
    step("GET /sign_up (signed in)", res)
    res, _ = alice.visit("POST", "/sign_in", {"email": "alice@example.com", "password": PASSWORD})
    step("POST /sign_in (signed in)", res)
    res, _ = alice.visit("GET", "/")
    step("GET / (signed in)", res)
    for path in ("/dashboard", "/settings/profile", "/settings/password", "/settings/email", "/settings/appearance", "/settings/sessions"):
        res, _ = alice.visit("GET", path)
        step(f"GET {path}", res)
    r = alice.html("/dashboard")
    step("GET /dashboard (html)", {"status": r.status_code, "head": head_of(r.text)})

    # ---- profile
    res, _ = alice.visit("PATCH", "/settings/profile", {"name": ""})
    step("PATCH /settings/profile blank", res)
    res, _ = alice.visit("PATCH", "/settings/profile", {"name": "Alice Liddell"})
    step("PATCH /settings/profile valid", res)
    res, _ = alice.visit("PATCH", "/settings/profile", {})
    step("PATCH /settings/profile no params", res)

    # ---- email
    res, _ = alice.visit("PATCH", "/settings/email", {"email": "alice2@example.com", "password_challenge": "wrong"})
    step("PATCH /settings/email wrong challenge", res)
    res, _ = alice.visit("PATCH", "/settings/email", {"email": "alice2@example.com"})
    step("PATCH /settings/email missing challenge", res)
    res, _ = alice.visit("PATCH", "/settings/email", {"email": "bob@example.com", "password_challenge": PASSWORD})
    step("PATCH /settings/email taken", res)
    res, _ = alice.visit("PATCH", "/settings/email", {"email": "bad", "password_challenge": PASSWORD})
    step("PATCH /settings/email invalid", res)
    res, _ = alice.visit("PATCH", "/settings/email", {"email": "alice@example.com", "password_challenge": PASSWORD})
    step("PATCH /settings/email unchanged", res, mails=mails(0))
    res, _ = alice.visit("PATCH", "/settings/email", {"email": "Alice2@Example.com", "password_challenge": PASSWORD})
    m = mails(1)
    change_link = public_link(m[0]) if m else None
    step("PATCH /settings/email changed", res, mails=m,
         verified=db("SELECT verified FROM users WHERE email = 'alice2@example.com'"))

    # ---- email verification
    alice.referer = base + "/settings/email"
    res, _ = alice.visit("POST", "/identity/email_verification")
    m = mails(1)
    resend_link = public_link(m[0]) if m else None
    step("POST /identity/email_verification (resend, referer)", res, mails=m)
    alice.referer = None
    res, _ = alice.visit("POST", "/identity/email_verification")
    mails(1)
    step("POST /identity/email_verification (resend, no referer)", res)
    res, _ = alice.visit("GET", "/identity/email_verification?sid=garbage")
    step("GET /identity/email_verification bad sid (signed in)", res)
    res, _ = guest.visit("GET", "/identity/email_verification?sid=garbage")
    step("GET /identity/email_verification bad sid (guest)", res)
    res, _ = guest.visit("GET", "/identity/email_verification")
    step("GET /identity/email_verification no sid (guest)", res)
    res, _ = guest.visit("GET", resend_link)
    step("GET verification link (guest)", res, verified=db("SELECT verified FROM users WHERE email = 'alice2@example.com'"))
    res, _ = guest.visit("GET", change_link)
    step("GET verification link again (still valid)", res)
    res, _ = guest.visit("POST", "/identity/email_verification")
    step("POST /identity/email_verification (guest)", res)

    # ---- password
    res, _ = alice.visit("PATCH", "/settings/password", {"password": "newpassword123", "password_confirmation": "newpassword123", "password_challenge": "wrong"})
    step("PATCH /settings/password wrong challenge", res)
    res, _ = alice.visit("PATCH", "/settings/password", {"password": "short", "password_confirmation": "nope", "password_challenge": PASSWORD})
    step("PATCH /settings/password short + mismatch", res)
    res, _ = alice.visit("PATCH", "/settings/password", {"password": "", "password_confirmation": "", "password_challenge": PASSWORD})
    step("PATCH /settings/password blank", res)

    alice_other = Browser()
    alice_other.html("/sign_in")
    res, _ = alice_other.visit("POST", "/sign_in", {"email": "alice2@example.com", "password": PASSWORD})
    step("POST /sign_in second browser", res)
    res, _ = alice.visit("GET", "/settings/sessions")
    step("GET /settings/sessions (two sessions)", res)
    NEW_PASSWORD = "newpassword123"
    res, _ = alice.visit("PATCH", "/settings/password", {"password": NEW_PASSWORD, "password_confirmation": NEW_PASSWORD, "password_challenge": PASSWORD})
    step("PATCH /settings/password valid", res)
    res, _ = alice_other.visit("GET", "/dashboard")
    step("GET /dashboard other browser after password change", res)
    res, _ = alice.visit("GET", "/settings/sessions")
    step("GET /settings/sessions after password change", res)

    # ---- sessions: log out another session, a foreign one, then this one
    alice_other = Browser()
    alice_other.html("/sign_in")
    alice_other.visit("POST", "/sign_in", {"email": "alice2@example.com", "password": NEW_PASSWORD})
    _, page = alice.visit("GET", "/settings/sessions")
    mine = page["props"]["auth"]["session"]["id"]
    others = [s["id"] for s in page["props"]["sessions"] if s["id"] != mine]
    res, _ = alice.visit("DELETE", f"/sessions/{others[0]}")
    step("DELETE /sessions/:other", res)
    res, _ = alice_other.visit("GET", "/dashboard")
    step("GET /dashboard logged-out browser", res)
    _, bob_page = bob.visit("GET", "/settings/sessions")
    bob_session = bob_page["props"]["auth"]["session"]["id"]
    res, _ = alice.visit("DELETE", f"/sessions/{bob_session}")
    step("DELETE /sessions/:foreign", {k: v for k, v in res.items() if k != "body"}, body_type=res.get("body", {}).get("type"))
    res, _ = alice.visit("DELETE", f"/sessions/{mine}")
    step("DELETE /sessions/:current", res)
    res, _ = alice.visit("GET", "/dashboard")
    step("GET /dashboard after deleting current session", res)

    # ---- sign in
    res, _ = alice.visit("POST", "/sign_in", {"email": "alice2@example.com", "password": "wrong"})
    step("POST /sign_in wrong password", res)
    res, _ = alice.visit("POST", "/sign_in", {"email": "nobody@example.com", "password": NEW_PASSWORD})
    step("POST /sign_in unknown email", res)
    res, _ = alice.visit("POST", "/sign_in", {"email": " ALICE2@example.com ", "password": NEW_PASSWORD})
    step("POST /sign_in right (unnormalized email)", res)

    # ---- password reset
    reset = Browser()
    reset.html("/identity/password_reset/new")
    res, _ = reset.visit("POST", "/identity/password_reset", {"email": "bob@example.com"})
    step("POST /identity/password_reset unverified", res, mails=mails(0))
    res, _ = reset.visit("POST", "/identity/password_reset", {"email": "nobody@example.com"})
    step("POST /identity/password_reset unknown", res, mails=mails(0))
    res, _ = reset.visit("POST", "/identity/password_reset", {"email": "alice2@example.com"})
    m = mails(1)
    reset_link = public_link(m[0]) if m else None
    step("POST /identity/password_reset verified", res, mails=m)
    res, _ = reset.visit("GET", "/identity/password_reset/edit?sid=garbage")
    step("GET /identity/password_reset/edit bad sid", res)
    res, _ = reset.visit("GET", reset_link)
    step("GET reset link", res)
    sid = urllib.parse.parse_qs(urllib.parse.urlsplit(reset_link).query)["sid"][0]
    res, _ = reset.visit("PATCH", "/identity/password_reset", {"sid": sid, "password": "resetpassword1", "password_confirmation": "mismatch"})
    step("PATCH /identity/password_reset mismatch", res)
    res, _ = reset.visit("PATCH", "/identity/password_reset", {"sid": sid, "password": "short", "password_confirmation": "short"})
    step("PATCH /identity/password_reset short", res)
    res, _ = reset.visit("PATCH", "/identity/password_reset", {"sid": "garbage", "password": "resetpassword1", "password_confirmation": "resetpassword1"})
    step("PATCH /identity/password_reset bad sid", res)
    res, _ = reset.visit("PATCH", "/identity/password_reset", {"sid": sid, "password": "resetpassword1", "password_confirmation": "resetpassword1"})
    step("PATCH /identity/password_reset valid", res)
    res, _ = alice.visit("GET", "/dashboard")
    step("GET /dashboard signed-in browser after reset", res)
    res, _ = reset.visit("GET", reset_link)
    step("GET reset link after use", res)
    res, _ = reset.visit("POST", "/sign_in", {"email": "alice2@example.com", "password": "resetpassword1"})
    step("POST /sign_in with reset password", res)

    # ---- delete account
    res, _ = reset.visit("DELETE", "/users", {"password_challenge": "wrong"})
    step("DELETE /users wrong challenge", res)
    res, _ = reset.visit("DELETE", "/users", {})
    step("DELETE /users missing challenge", res)
    res, _ = reset.visit("DELETE", "/users", {"password_challenge": "resetpassword1"})
    step("DELETE /users right challenge", res, users=db("SELECT email FROM users ORDER BY email"),
         sessions_left=db("SELECT COUNT(*) FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.email = 'alice2@example.com'"))
    res, _ = reset.visit("GET", "/dashboard")
    step("GET /dashboard after delete", res)

    # ---- pages the Rails kit renders without resolving the session (skip_before_action)
    for path in ("/identity/password_reset/new",):
        res, _ = bob.visit("GET", path)
        step(f"GET {path} (signed in)", res)

    # ---- sign out via the user menu path: DELETE /sessions/:id is the only sign-out
    res, _ = bob.visit("GET", "/settings/sessions")
    step("GET /settings/sessions (bob)", res)
    sess = db("SELECT user_agent, ip_address FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.email = 'bob@example.com'")
    step("session record (bob)", {"rows": sess})

    # ---- explicit JSON nulls (a hand-written client, not the kit's forms), on their own user
    nulls = Browser()
    nulls.html("/sign_up")
    res, _ = nulls.visit("POST", "/sign_up", {"name": None, "email": None, "password": None, "password_confirmation": None})
    step("POST /sign_up all null", {k: v for k, v in res.items() if k != "body"}, body_type=res.get("body", {}).get("type"))
    res, _ = nulls.visit("POST", "/sign_in", {"email": None, "password": None})
    step("POST /sign_in all null", {k: v for k, v in res.items() if k != "body"}, body_type=res.get("body", {}).get("type"))
    nulls.visit("POST", "/sign_up", {"name": "Carol", "email": "carol@example.com", "password": PASSWORD, "password_confirmation": PASSWORD})
    mails(1)
    res, _ = nulls.visit("PATCH", "/settings/profile", {"name": None})
    step("PATCH /settings/profile null name", res)
    res, _ = nulls.visit("PATCH", "/settings/password", {"password": None, "password_challenge": PASSWORD})
    step("PATCH /settings/password null password", res)
    res, _ = nulls.visit("PATCH", "/settings/email", {"email": "carol2@example.com", "password_challenge": None})
    step("PATCH /settings/email null challenge", res, emails=db("SELECT email FROM users WHERE email LIKE 'carol%'"))


try:
    flows()
finally:
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(steps, f, indent=2, sort_keys=True, ensure_ascii=False)
    print(f"{len(steps)} steps -> {out_path}")
