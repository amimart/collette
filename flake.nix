{
  description = "Collette development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
  };

  outputs =
    { nixpkgs, ... }:
    let
      supportedSystems = [
        "aarch64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo-audit
              pkgs.cargo-binstall
              pkgs.cargo-llvm-cov
              pkgs.deadnix
              pkgs.gawk
              pkgs.gettext
              pkgs.git
              pkgs.gnumake
              pkgs.ncurses
              pkgs.nixfmt
              pkgs.nodejs_22
              pkgs.rust-analyzer
              pkgs.rustup
              pkgs.statix
              pkgs.yamllint
            ];

            shellHook = ''
              echo "Collette development environment loaded"
            '';
          };
        }
      );
    };
}
