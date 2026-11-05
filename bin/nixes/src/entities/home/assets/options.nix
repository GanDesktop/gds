{ options, config, ... }:
let
  safe =
    value:
    let
      result = builtins.tryEval (builtins.deepSeq value value);
    in
    if result.success then
      {
        available = true;
        value = result.value;
      }
    else
      {
        available = false;
        value = null;
      };
  # Do not serialize derivations, functions or whole submodules as form values.
  scalar =
    value:
    builtins.isString value
    || builtins.isBool value
    || builtins.isInt value
    || builtins.isFloat value
    || value == null;
  present =
    value:
    if scalar value then
      {
        kind = "scalar";
        data = value;
      }
    else if builtins.isPath value then
      {
        kind = "scalar";
        data = toString value;
      }
    else if builtins.isList value then
      {
        kind = "list";
        data = map (
          item:
          if scalar item then
            item
          else if builtins.isAttrs item && item ? name then
            item.name
          else
            "…"
        ) value;
      }
    else if builtins.isAttrs value then
      {
        kind = "attributes";
        data = builtins.attrNames value;
      }
    else
      {
        kind = "unsupported";
        data = null;
      };
  at =
    path: root:
    builtins.foldl' (
      value: key:
      (
        if builtins.isAttrs value && builtins.hasAttr key value then
          value.${key}
        else
          throw "Option has no value"
      )
    ) root path;
  unavailable = {
    available = false;
    value = null;
  };
  homeFields = [
    "username"
    "homeDirectory"
    "stateVersion"
    "packages"
    "sessionVariables"
  ];
  isHomeField =
    path:
    builtins.length path == 2
    && builtins.head path == "home"
    && builtins.elem (builtins.elemAt path 1) homeFields;
  # Prune disabled branches before touching their descriptions, defaults or values.
  walk =
    path: tree: inherited:
    let
      hasEnable = (tree.enable._type or "") == "option";
      enabled =
        if hasEnable then
          builtins.tryEval (at (path ++ [ "enable" ]) config)
        else
          {
            success = true;
            value = inherited;
          };
      active = enabled.success && enabled.value == true;
    in
    if hasEnable && !active then
      [ ]
    else
      builtins.concatMap (
        key:
        let
          node = tree.${key};
          next = path ++ [ key ];
        in
        if builtins.substring 0 1 key == "_" then
          [ ]
        else if (node._type or "") == "option" then
          if !(active || isHomeField next) || (node.internal or false) || (node.visible or true) == false then
            [ ]
          else
            [
              {
                path = next;
                type = node.type.name or "unknown";
                description =
                  if builtins.isString (node.description or null) then
                    node.description
                  else
                    (node.description.text or "");
                readOnly = node.readOnly or false;
                current = safe (present (at next config));
                default = if node ? default then safe (present node.default) else unavailable;
              }
            ]
        else if builtins.isAttrs node then
          walk next node active
        else
          [ ]
      ) (builtins.attrNames tree);
in
walk [ ] options false
