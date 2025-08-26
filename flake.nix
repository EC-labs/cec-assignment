{
    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/25.05";
        crate2nix = {
            url = "github:landaudiogo/crate2nix";
            inputs.nixpkgs.follows = "nixpkgs";
        };
    };

    outputs = { self, nixpkgs, crate2nix, ... }@inputs:
        let 
            system = "x86_64-linux";
            pkgs = nixpkgs.legacyPackages.${system};
            crate2nixTools = crate2nix.lib.tools;
            crate = pkgs.callPackage (import ./default.nix) { inherit crate2nixTools; };
        in
        {
            devShells.${system} = {
                default = pkgs.callPackage (import ./shell.nix) {};
            };

            packages.${system} = {
                default = self.packages.${system}.producer;
                producer = crate.workspaceMembers.experiment-producer.build;
            };

            images.${system} = {
                producer = 
                    let 
                        env = pkgs.runCommand "schemas" {} ''
                            mkdir -p $out/experiment-producer
                            cp -r ${./experiment-producer/schemas} $out/experiment-producer/schemas
                            cp ${./experiment-producer/.env} $out/experiment-producer/.env
                        '';
                    in
                    pkgs.dockerTools.buildImage {
                        name = "experiment-producer";
                        tag = "latest";
                        copyToRoot = [ 
                            self.packages.${system}.producer 
                            env
                        ];
                        config = {
                            Entrypoint = [ "/bin/experiment-producer" ];
                        };
                    };
            };
        };
    }
