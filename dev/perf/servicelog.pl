# servicelog.pl <lines>: colored service-log lines (~100 characters: dim timestamp, colored
# level, dim module, recurring message templates with varying ids), as fast as the terminal
# reads them, then exit. Deterministic (fixed seed). Scenario I of sessions.sh.
use strict; use warnings;
my $n = shift // 1_050_000;
my $x = 7;
sub r { my $m = shift; $x = ($x * 1103515245 + 12345) % 2147483648; return int($x / 65536) % $m; }
my @mods = qw(api::handlers::items api::auth db::pool worker::queue cache::redis http::server billing::invoices search::index);
my $ms = 0; my $buf = '';
for my $i (1 .. $n) {
  $ms += r(900);
  my $w = r(20);
  my ($lvl, $c) = ($w == 15 || $w == 16 || $w == 19) ? ('ERROR', 31) : ($w == 6 || $w == 9 || $w == 11) ? (' WARN', 33) : (' INFO', 32);
  my $t = $w % 5;
  my $msg = $t == 0 ? sprintf('request id=%08x path=/api/v1/items/%d status=%d took=%dms', r(2**31), r(10000), (200, 201, 404, 500)[r(4)], r(900))
          : $t == 1 ? sprintf('connection %d returned to pool (idle=%d, busy=%d)', r(64), r(32), r(32))
          : $t == 2 ? sprintf('job %d finished in %d.%03ds, %d retries', r(1000000), r(30), r(1000), r(4))
          : $t == 3 ? sprintf('cache miss for key user:%d:profile, fetched in %dms', r(100000), r(50))
          :           sprintf('user %d signed in from 10.%d.%d.%d', r(100000), r(256), r(256), r(256));
  $buf .= sprintf("\e[2m2026-11-08T%02d:%02d:%02d.%03dZ\e[0m \e[%dm%s\e[0m \e[2m%s\e[0m: %s\n",
    int($ms / 3600000) % 24, int($ms / 60000) % 60, int($ms / 1000) % 60, $ms % 1000, $c, $lvl, $mods[r(8)], $msg);
  if (length($buf) > 65536) { print $buf; $buf = ''; }
}
print $buf;
