let
  order = [
    "hyprland"
    "weston"
  ];
  metadata = {
    hyprland = {
      label = "Hyprland";
      short = "H";
      description = "Hyprland VM -> virtio-gpu software KMS -> klocc-gui Wayland client -> grim -c capture";
    };
    weston = {
      label = "Weston/Xvfb";
      short = "W";
      description = "Xvfb -> Weston X11 backend -> klocc-gui Wayland client -> xwd/ImageMagick capture";
    };
  };
in {
  inherit order metadata;
  backends = {
    hyprland = import ./hyprland.nix;
    weston = import ./weston.nix;
  };
  review = map (id: metadata.${id} // {inherit id;}) order;
}
