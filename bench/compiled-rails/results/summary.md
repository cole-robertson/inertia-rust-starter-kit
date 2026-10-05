_5 runs; median (min–max) for req/s, medians for the rest._

### The real kits

| App | Endpoint | req/s | p50 ms | p99 ms | CPU µs/req |
|---|---|---:|---:|---:|---:|
| Rust kit (Loco) | `/up` | 95,808 (71,557–100,465) | 0.33 | 0.59 | 38 |
| Rust kit (Loco) | `/sign_in` HTML | 69,898 (59,061–80,167) | 0.43 | 0.89 | 51 |
| Rust kit (Loco) | `/sign_in` XHR | 75,923 (66,291–84,577) | 0.38 | 0.83 | 49 |
| Rust kit (Loco) | `/dashboard` HTML | 30,793 (22,961–37,762) | 1.03 | 1.93 | 124 |
| Rust kit (Loco) | `/dashboard` XHR | 33,533 (13,570–41,509) | 0.94 | 1.77 | 114 |
| Rails kit (stock, 4×3 Puma) | `/up` | 14,891 (13,606–17,911) | 1.81 | 7.64 | 258 |
| Rails kit (stock, 4×3 Puma) | `/sign_in` HTML | 4,435 (3,328–5,301) | 6.09 | 19.55 | 821 |
| Rails kit (stock, 4×3 Puma) | `/sign_in` XHR | 4,984 (4,372–6,342) | 5.04 | 24.14 | 779 |
| Rails kit (stock, 4×3 Puma) | `/dashboard` HTML | 2,972 (2,805–3,735) | 9.03 | 32.70 | 1,317 |
| Rails kit (stock, 4×3 Puma) | `/dashboard` XHR | 3,522 (3,242–4,381) | 7.94 | 28.91 | 1,107 |

### Stand-in slice (NOT the Inertia kit: see §3)

| App | Endpoint | req/s | p50 ms | p99 ms | CPU µs/req |
|---|---|---:|---:|---:|---:|
| Slice on stock Rails (4×3) | `/up` | 13,844 (11,680–14,810) | 1.79 | 10.25 | 279 |
| Slice on stock Rails (4×3) | `/sign_in` HTML | 6,403 (4,130–6,839) | 4.15 | 17.16 | 610 |
| Slice on stock Rails (4×3) | `/sign_in` XHR | 9,955 (7,852–10,676) | 2.33 | 14.94 | 390 |
| Slice on stock Rails (4×3) | `/dashboard` HTML | 4,593 (3,797–4,924) | 6.20 | 19.57 | 853 |
| Slice on stock Rails (4×3) | `/dashboard` XHR | 6,581 (4,393–7,067) | 4.07 | 16.59 | 595 |
| Slice, Roundhouse Ruby emit (4×3) | `/up` | 68,591 (52,136–72,341) | 0.40 | 1.49 | 43 |
| Slice, Roundhouse Ruby emit (4×3) | `/sign_in` HTML | 23,770 (21,952–31,637) | 1.13 | 3.43 | 122 |
| Slice, Roundhouse Ruby emit (4×3) | `/sign_in` XHR | 53,921 (45,664–63,860) | 0.54 | 1.67 | 55 |
| Slice, Roundhouse Ruby emit (4×3) | `/dashboard` HTML | 4,116 (3,018–5,738) | 6.90 | 24.98 | 942 |
| Slice, Roundhouse Ruby emit (4×3) | `/dashboard` XHR | 4,517 (3,208–6,570) | 5.61 | 24.50 | 865 |
| Slice, Spinel binary (4 workers) | `/up` | 111,652 (108,260–139,097) | 0.26 | 0.93 | 24 |
| Slice, Spinel binary (4 workers) | `/sign_in` HTML | 58,434 (45,374–69,816) | 0.45 | 3.10 | 48 |
| Slice, Spinel binary (4 workers) | `/sign_in` XHR | 87,055 (67,988–109,646) | 0.33 | 1.35 | 32 |
| Slice, Spinel binary (4 workers) | `/dashboard` HTML | 17,170 (15,826–21,176) | 1.60 | 5.09 | 187 |
| Slice, Spinel binary (4 workers) | `/dashboard` XHR | 17,292 (16,484–21,168) | 1.55 | 5.12 | 183 |

### Roundhouse blog fixture (upstream-supported reference)

| App | Endpoint | req/s | p50 ms | p99 ms | CPU µs/req |
|---|---|---:|---:|---:|---:|
| Blog on stock Rails (4×3) | `/articles` | 2,760 (2,272–2,781) | 11.05 | 28.35 | 1,428 |
| Blog on stock Rails (4×3) | `/articles/1` | 2,534 (1,124–2,660) | 12.08 | 28.09 | 1,552 |
| Blog, Roundhouse Ruby emit (4×3) | `/articles` | 26,984 (17,716–28,405) | 1.03 | 3.39 | 142 |
| Blog, Roundhouse Ruby emit (4×3) | `/articles/1` | 27,520 (15,618–29,622) | 0.98 | 3.56 | 139 |
| Blog, Spinel binary (4 workers) | `/articles` | 50,045 (28,326–52,111) | 0.58 | 2.12 | 62 |
| Blog, Spinel binary (4 workers) | `/articles/1` | 57,119 (42,699–59,656) | 0.48 | 2.54 | 52 |
| Blog, Roundhouse Rust emit (tokio, 4) | `/articles` | 74,715 (56,080–79,994) | 0.43 | 0.75 | 49 |
| Blog, Roundhouse Rust emit (tokio, 4) | `/articles/1` | 113,704 (79,787–117,179) | 0.28 | 0.49 | 32 |

### Boot and memory

| App | Boot to first 200 | RSS idle | RSS after load |
|---|---:|---:|---:|
| Rust kit (Loco) | 32 (31–61) ms | 68 (68–71) MiB | 67 (57–71) MiB |
| Rails kit (stock, 4×3 Puma) | 1,063 (1,059–1,073) ms | 627 (606–627) MiB | 874 (864–896) MiB |
| Slice on stock Rails (4×3) | 1,027 (1,023–1,045) ms | 602 (578–622) MiB | 879 (839–906) MiB |
| Slice, Roundhouse Ruby emit (4×3) | 295 (293–309) ms | 231 (229–235) MiB | 376 (367–401) MiB |
| Slice, Spinel binary (4 workers) | 8 (7–8) ms | 29 (29–31) MiB | 54 (52–56) MiB |
| Blog on stock Rails (4×3) | 1,119 (1,117–1,429) ms | 691 (690–693) MiB | 849 (846–859) MiB |
| Blog, Roundhouse Ruby emit (4×3) | 457 (408–472) ms | 246 (246–247) MiB | 342 (338–344) MiB |
| Blog, Spinel binary (4 workers) | 41 (39–44) ms | 29 (29–31) MiB | 61 (58–62) MiB |
| Blog, Roundhouse Rust emit (tokio, 4) | 7 (6–8) ms | 9 (9–9) MiB | 11 (11–11) MiB |

_1-min load average sampled after each lane: median 9.1 (range 4.5–14.1); the app used 4 cores and oha 8._
