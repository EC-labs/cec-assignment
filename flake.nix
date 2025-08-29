{
    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/25.05";
        crate2nix = {
            url = "github:landaudiogo/crate2nix";
            inputs.nixpkgs.follows = "nixpkgs";
        };

        pyproject-nix = {
            url = "github:pyproject-nix/pyproject.nix";
            inputs.nixpkgs.follows = "nixpkgs";
        };

        uv2nix = {
            url = "github:pyproject-nix/uv2nix";
            inputs.pyproject-nix.follows = "pyproject-nix";
            inputs.nixpkgs.follows = "nixpkgs";
        };

        pyproject-build-systems = {
            url = "github:pyproject-nix/build-system-pkgs";
            inputs.pyproject-nix.follows = "pyproject-nix";
            inputs.uv2nix.follows = "uv2nix";
            inputs.nixpkgs.follows = "nixpkgs";
        };
    };

    outputs = { 
        self, 
        nixpkgs, 
        crate2nix, 
        uv2nix,
        pyproject-nix,
        pyproject-build-systems,
        ... 
    }@inputs:
        let 
            system = "x86_64-linux";
            pkgs = nixpkgs.legacyPackages.${system};
            inherit (nixpkgs) lib;

            crate2nixTools = crate2nix.lib.tools;
            crate = pkgs.callPackage (import ./default.nix) { inherit crate2nixTools; };

            workspace = uv2nix.lib.workspace.loadWorkspace { workspaceRoot = ./production-rate; };
            overlay = workspace.mkPyprojectOverlay {
                sourcePreference = "wheel"; # or sourcePreference = "sdist";
            };

            python = pkgs.python312;

            pythonSet = (pkgs.callPackage pyproject-nix.build.packages { inherit python; }).overrideScope
                (
                    lib.composeManyExtensions [
                        pyproject-build-systems.overlays.default
                        overlay
                    ]
                );
            venv = pythonSet.mkVirtualEnv "production-rate-env" workspace.deps.default;
        in
        {
            devShells.${system} = {
                default = pkgs.mkShell {
                    packages = with pkgs; [
                        cargo
                        rustc
                        rust-analyzer
                        pkg-config
                        openssl
                        cmake
                    ];
                };
                production-rate = pkgs.mkShell {
                    packages = [
                        python
                        pkgs.uv
                    ];
                    env =
                        {
                            UV_PYTHON_DOWNLOADS = "never";
                            UV_PYTHON = python.interpreter;
                        }
                        // lib.optionalAttrs pkgs.stdenv.isLinux {
                            LD_LIBRARY_PATH = lib.makeLibraryPath pkgs.pythonManylinuxPackages.manylinux1;
                        };
                    shellHook = ''
                        unset PYTHONPATH
                    '';
                };
            };

            packages.${system} = {
                default = self.packages.${system}.experiment-producer;
                experiment-producer = crate.workspaceMembers.experiment-producer.build;
                notifications-service = crate.workspaceMembers.notifications-service.build;
                production-rate = pkgs.writeShellScriptBin "production-rate" ''
                    source ${venv}/bin/activate
                    ${venv}/bin/production-rate "$@"
                '';
            };

            images.${system} = {
                experiment-producer = 
                    let 
                        env = pkgs.runCommand "schemas" {} ''
                            mkdir -p $out/experiment-producer
                            cp -r ${./experiment-producer/schemas} $out/experiment-producer/schemas
                            cp ${./experiment-producer/.env} $out/experiment-producer/.env
                        '';
                    in
                    pkgs.dockerTools.buildImage {
                        name = "dclandau/cec-experiment-producer";
                        tag = "latest";
                        copyToRoot = [ 
                            self.packages.${system}.experiment-producer 
                            env
                        ];
                        config = {
                            Entrypoint = [ "/bin/experiment-producer" ];
                        };
                    };
                notifications-service = 
                    let 
                        env = pkgs.runCommand "schemas" {} ''
                            mkdir -p $out/notifications-service
                            cp ${./notifications-service/.env} $out/notifications-service/.env
                        '';
                    in
                    pkgs.dockerTools.buildImage {
                        name = "dclandau/cec-notifications-service";
                        tag = "latest";
                        copyToRoot = [ 
                            self.packages.${system}.notifications-service
                            env
                        ];
                        config = {
                            Entrypoint = [ "/bin/notifications-service" ];
                        };
                    };
                production-rate = 
                    pkgs.dockerTools.buildImage {
                        name = "dclandau/cec-production-rate";
                        tag = "latest";
                        copyToRoot = [ 
                            pkgs.coreutils
                            self.packages.${system}.production-rate
                        ];
                        config = {
                            Entrypoint = [ "/bin/production-rate" ];
                        };
                    };
            };

            apps.${system} = {
                pushImages = 
                    let
                        loadPush = imageDerivation: ''
                            image=$(docker load < ${imageDerivation} | sed -nE 's/Loaded image: (\w+)/\1/p')
                            docker push $image
                        '';
                        joined = 
                            builtins.concatStringsSep "\n" (
                                builtins.map loadPush (builtins.attrValues self.images.${system})
                            );
                        scriptContents = 
                            ''
                            set -e
                            '' 
                            + joined;
                        script = pkgs.writeShellScript "push-images" scriptContents;
                    in
                    {
                        type = "app";
                        program = "${script}"; 
                    };
            };
        };
    }
