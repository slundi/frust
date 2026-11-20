# NixOS module: generates the frust YAML config and runs the aggregator
# periodically with a systemd timer (the NixOS equivalent of a cron job).
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.frust;
  yaml = pkgs.formats.yaml { };

in
{
  options.services.frust = {
    enable = lib.mkEnableOption "frust, the RSS/Atom feed aggregator";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The frust package to use.";
    };

    settings = lib.mkOption {
      type = yaml.type;
      default = { };
      description = ''
        frust configuration, written as YAML. See `my-config.yaml` for the
        available keys. `output` and `data_dir` are set from
        {option}`services.frust.outputDir` and {option}`services.frust.dataDir`.
      '';
      example = lib.literalExpression ''
        {
          base_url = "https://example.org/rss";
          retention = 60;
          filters = [
            { slug = "no-drums"; expressions = [ "drum" ]; keep = false; }
          ];
          groups = [
            {
              title = "Rust";
              slug = "rust";
              output = "rust.atom";
              feeds = [
                { title = "This Week in Rust"; url = "https://this-week-in-rust.org/atom.xml"; }
              ];
            }
          ];
        }
      '';
    };

    configFile = lib.mkOption {
      type = lib.types.path;
      readOnly = true;
      # `output` and `data_dir` come from the module options so the service
      # sandbox and the config can't disagree on where frust writes.
      default = yaml.generate "frust.yaml" (
        cfg.settings
        // {
          output = cfg.outputDir;
          data_dir = cfg.dataDir;
        }
      );
      defaultText = lib.literalMD "generated from {option}`services.frust.settings`";
      description = "The generated YAML config, e.g. for `frust export out.opml <configFile>`.";
    };

    outputDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/www/frust";
      description = "Directory of the generated feeds, to be served by the web server.";
    };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/frust";
      description = "Private directory of the databases. Must not be inside {option}`outputDir`.";
    };

    schedule = lib.mkOption {
      type = lib.types.str;
      default = "hourly";
      example = "*-*-* 00/2:00:00";
      description = "When to run frust, in {manpage}`systemd.time(7)` calendar format.";
    };

    randomizedDelay = lib.mkOption {
      type = lib.types.str;
      default = "5m";
      description = "Random delay added to each run so feeds aren't all hit on the hour.";
    };

    logLevel = lib.mkOption {
      type = lib.types.str;
      default = "info";
      example = "frust_feed=debug,warn";
      description = "`RUST_LOG` directive.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "frust";
      description = "User running frust.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "frust";
      description = "Group owning the output directory. Add the web server user to it if needed.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !lib.hasPrefix "${toString cfg.outputDir}/" "${toString cfg.dataDir}/";
        message = "services.frust.dataDir must not be inside services.frust.outputDir: the databases would be publicly served.";
      }
    ];

    users.users = lib.mkIf (cfg.user == "frust") {
      frust = {
        isSystemUser = true;
        inherit (cfg) group;
        home = cfg.dataDir;
      };
    };
    users.groups = lib.mkIf (cfg.group == "frust") { frust = { }; };

    systemd.tmpfiles.settings."10-frust" = {
      # frust re-applies 0700 on creation; tmpfiles only guarantees ownership.
      ${cfg.dataDir}.d = {
        inherit (cfg) user group;
        mode = "0700";
      };
      # World-readable: everything in it is meant to be published.
      ${cfg.outputDir}.d = {
        inherit (cfg) user group;
        mode = "0755";
      };
    };

    systemd.services.frust = {
      description = "frust feed aggregator";
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      environment.RUST_LOG = cfg.logLevel;
      # Restarting on config change would trigger an unscheduled fetch.
      restartIfChanged = false;

      serviceConfig = {
        Type = "oneshot";
        User = cfg.user;
        Group = cfg.group;
        ExecStart = "${lib.getExe' cfg.package "frust"} --config ${cfg.configFile}";
        WorkingDirectory = cfg.dataDir;
        UMask = "0022";

        # Hardening: frust only needs the network and its two directories.
        ReadWritePaths = [
          cfg.dataDir
          cfg.outputDir
        ];
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        NoNewPrivileges = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        CapabilityBoundingSet = "";
        SystemCallArchitectures = "native";
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
        ];
      };
    };

    systemd.timers.frust = {
      description = "Run frust periodically";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = cfg.schedule;
        RandomizedDelaySec = cfg.randomizedDelay;
        # Catch up on a missed run after downtime (e.g. a router reboot).
        Persistent = true;
      };
    };
  };
}
