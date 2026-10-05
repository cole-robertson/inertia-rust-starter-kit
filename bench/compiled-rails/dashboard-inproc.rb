# In-process timing of one signed-in /dashboard on the Roundhouse Ruby emit (Rack::Test, YJIT).
# Used to find the Base64.char_value hotspot: run from out/slice-ruby with `bundle exec ruby`.
require "rack"; require "rack/builder"; require "rack/test"
app, _ = Rack::Builder.parse_file("config.ru")
include Rack::Test::Methods
define_method(:app) { app }
ua = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36"
header "User-Agent", ua
get "/sign_in"; tok = last_response.body[/csrf-token" content="([^"]+)/, 1]
header "X-CSRF-Token", tok
post "/sign_in", "email=bench@example.com&password=bench-password-1"
puts last_response.status
get "/dashboard"; puts last_response.status
n=300
t=(s=Process.clock_gettime(Process::CLOCK_MONOTONIC); n.times { get "/dashboard" }; Process.clock_gettime(Process::CLOCK_MONOTONIC)-s); puts "dashboard #{(t/n*1e6).round} us"
t=(s=Process.clock_gettime(Process::CLOCK_MONOTONIC); n.times { get "/up" }; Process.clock_gettime(Process::CLOCK_MONOTONIC)-s); puts "up #{(t/n*1e6).round} us"
n=2000; 200.times { get "/dashboard" }
s=Process.clock_gettime(Process::CLOCK_MONOTONIC); n.times { get "/dashboard" }; puts "dashboard #{((Process.clock_gettime(Process::CLOCK_MONOTONIC)-s)/n*1e6).round} us/req (in-process, Rack::Test, 1 thread)"
