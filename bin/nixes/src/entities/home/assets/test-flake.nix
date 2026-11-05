{
  outputs =
    { self }:
    let
      option = type: {
        _type = "option";
        inherit type;
        description = "Example";
      };
      options = {
        programs.example.enable = (option { name = "bool"; }) // {
          default = false;
        };
        programs.example.path = option { name = "path"; };
        programs.disabled.enable = option { name = "bool"; };
        programs.disabled.child.enable = option { name = "bool"; };
        programs.disabled.value = (option { name = "str"; }) // {
          default = builtins.head [ ];
        };
        programs.example.missing = option { name = "str"; };
        programs.example.settings = option { name = "attrs"; };
        programs.example.nested.enable = option { name = "bool"; };
        programs.example.nested.value = (option { name = "str"; }) // {
          description = builtins.head [ ];
        };
        programs.unmanaged.value = option { name = "str"; };
        hidden = (option { name = "str"; }) // {
          internal = true;
        };
      };
      config = {
        programs.example.enable = true;
        programs.example.path = ./.;
        programs.disabled.enable = false;
        programs.disabled.child.enable = true;
        programs.disabled.value = builtins.head [ ];
        programs.example.missing = throw "unavailable option";
        programs.example.settings.editor = "hx";
        programs.example.nested.enable = false;
        programs.unmanaged.value = "not enabled";
      };
      user = "quoted\"\${throw \"must remain literal\"}";
    in
    {
      homeConfigurations.${user} = {
        inherit options config;
        activationPackage = null;
      };
    };
}
