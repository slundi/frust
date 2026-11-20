# End-to-end test of the NixOS module: boot a VM, serve a fixture feed with
# nginx, run the service and check what gets published.
{ pkgs, module }:
let
  # `@NOW@` is replaced at test time: an old entry would fall outside the
  # retention window.
  fixture = pkgs.writeText "fixture.atom" ''
    <?xml version="1.0" encoding="utf-8"?>
    <feed xmlns="http://www.w3.org/2005/Atom">
      <title>Fixture</title>
      <id>urn:frust:fixture</id>
      <updated>@NOW@</updated>
      <link href="https://example.org/"/>
      <entry>
        <title>Fixture article</title>
        <id>urn:frust:fixture:1</id>
        <link href="https://example.org/1"/>
        <updated>@NOW@</updated>
        <summary>Served by the NixOS VM test.</summary>
      </entry>
    </feed>
  '';
in
pkgs.testers.runNixOSTest {
  name = "frust";

  nodes.machine = {
    imports = [ module ];

    services.frust = {
      enable = true;
      settings.groups = [
        {
          title = "Local";
          slug = "local";
          output = "local.atom";
          feeds = [
            {
              title = "Fixture";
              # Feed URLs from the config may be loopback; only URLs taken
              # from feed content go through the SSRF guard.
              url = "http://127.0.0.1:8080/fixture.atom";
            }
          ];
        }
      ];
    };

    services.nginx = {
      enable = true;
      virtualHosts.fixture = {
        listen = [
          {
            addr = "127.0.0.1";
            port = 8080;
          }
        ];
        root = "/srv/fixture";
      };
      virtualHosts.public = {
        default = true;
        locations."/rss/".alias = "/var/www/frust/";
      };
    };

    environment.systemPackages = [ pkgs.curl ];
  };

  testScript = ''
    machine.wait_for_unit("nginx.service")
    machine.wait_for_unit("frust.timer")
    machine.succeed(
        "mkdir -p /srv/fixture"
        " && sed \"s/@NOW@/$(date -u +%FT%TZ)/\" ${fixture} > /srv/fixture/fixture.atom"
    )

    with subtest("a run publishes the feed"):
        # Oneshot: blocks until frust exits, fails on a non-zero exit code.
        machine.succeed("systemctl start frust.service")
        machine.succeed("grep -q 'Fixture article' /var/www/frust/local.atom")
        machine.succeed("curl -sf http://localhost/rss/local.atom | grep -q 'Fixture article'")

    with subtest("databases stay private"):
        machine.succeed("test \"$(stat -c %a /var/lib/frust)\" = 700")
        machine.succeed("test -f /var/lib/frust/articles.redb")
        machine.fail("ls /var/www/frust/*.redb")

    with subtest("a second run succeeds"):
        machine.succeed("systemctl start frust.service")
        machine.succeed("grep -q 'Fixture article' /var/www/frust/local.atom")
  '';
}
