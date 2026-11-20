# Fast checks of the NixOS module, no VM: evaluate a few configurations,
# assert on the result, then validate the generated YAML with frust itself.
{
  pkgs,
  nixosSystem,
  module,
}:
let
  inherit (pkgs) lib;

  evalWith =
    frust:
    (nixosSystem {
      modules = [
        module
        {
          nixpkgs.hostPlatform = pkgs.stdenv.hostPlatform.system;
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "/dev/null";
            fsType = "ext4";
          };
          system.stateVersion = lib.trivial.release;
          services.frust = {
            enable = true;
          }
          // frust;
        }
      ];
    }).config;

  failedAssertions =
    config:
    map (assertion: assertion.message) (lib.filter (assertion: !assertion.assertion) config.assertions);

  example = evalWith {
    schedule = "*-*-* 00/2:00:00";
    settings = {
      # Overridden by `outputDir`: the sandbox only allows writing there.
      output = "/ignored";
      retention = 60;
      groups = [
        {
          title = "Rust";
          slug = "rust";
          output = "rust.atom";
          feeds = [
            {
              title = "This Week in Rust";
              url = "https://this-week-in-rust.org/atom.xml";
            }
          ];
        }
      ];
    };
  };
  service = example.systemd.services.frust.serviceConfig;

  failures = lib.runTests {
    testSchedule = {
      expr = example.systemd.timers.frust.timerConfig.OnCalendar;
      expected = "*-*-* 00/2:00:00";
    };
    testTimerEnabled = {
      expr = example.systemd.timers.frust.wantedBy;
      expected = [ "timers.target" ];
    };
    testServiceUsesGeneratedConfig = {
      expr = lib.hasSuffix "--config ${example.services.frust.configFile}" service.ExecStart;
      expected = true;
    };
    testWritablePaths = {
      expr = service.ReadWritePaths;
      expected = [
        "/var/lib/frust"
        "/var/www/frust"
      ];
    };
    testDefaultUserCreated = {
      expr = example.users.users.frust.isSystemUser;
      expected = true;
    };
    testCustomUserNotCreated = {
      expr = (evalWith { user = "feeds"; }).users.users ? frust;
      expected = false;
    };
    testValidLayout = {
      expr = failedAssertions example;
      expected = [ ];
    };
    testDataDirInsideOutputRejected = {
      expr = lib.length (
        failedAssertions (evalWith {
          dataDir = "/var/www/frust/db";
        })
      );
      expected = 1;
    };
    testSiblingWithSamePrefixAllowed = {
      expr = failedAssertions (evalWith {
        dataDir = "/var/www/frust-db";
      });
      expected = [ ];
    };
  };
in
if failures != [ ] then
  throw "frust module eval tests failed: ${builtins.toJSON failures}"
else
  pkgs.runCommand "frust-module-eval"
    {
      nativeBuildInputs = [
        example.services.frust.package
        pkgs.yq-go
      ];
    }
    ''
      config=${example.services.frust.configFile}
      yq -e '.output == "/var/www/frust" and .data_dir == "/var/lib/frust"' "$config"
      # Loads and validates the config (panics on invalid data) without fetching.
      frust export feeds.opml "$config"
      grep -q 'this-week-in-rust.org/atom.xml' feeds.opml
      touch $out
    ''
