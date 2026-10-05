// Mock upstream for the I/O benchmark (docs/BENCHMARK.md): GET /slow?ms=N sleeps N ms, then
// answers a small JSON body. Go's net/http serves each connection on its own goroutine, so
// 512+ concurrent sleeping requests cost no CPU; bench/io-run.sh checks it is never the
// bottleneck by hitting it directly first.
package main

import (
	"fmt"
	"log"
	"net/http"
	"os"
	"strconv"
	"time"
)

func main() {
	addr := os.Getenv("ADDR")
	if addr == "" {
		addr = "0.0.0.0:9900"
	}
	http.HandleFunc("/slow", func(w http.ResponseWriter, r *http.Request) {
		ms, err := strconv.Atoi(r.URL.Query().Get("ms"))
		if err != nil || ms < 0 || ms > 60000 {
			http.Error(w, "ms must be 0..60000", http.StatusBadRequest)
			return
		}
		time.Sleep(time.Duration(ms) * time.Millisecond)
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprintf(w, `{"slept_ms":%d}`, ms)
	})
	srv := &http.Server{Addr: addr, ReadHeaderTimeout: 10 * time.Second}
	log.Printf("mock upstream on %s", addr)
	log.Fatal(srv.ListenAndServe())
}
