# Front-panel ports from cages + explicit breakout overrides (pure; used by
# default.nix and the eval tests). Rules: config-ownership.md "ports".
#   SONiC name from the port's first lane, as SONiC does:
#   Ethernet<stride*(cage-1) + index of the first lane within the cage (0-based)>,
#   stride = lanes of the widest cage (QSFP28: 4, QSFP-DD/OSFP: 8);
#   alias = Ethernet<cage>/<subport>.
{ lib }:
{ cages, breakout, defaultSpeed, validSpeeds, allowUnusedLanes ? false }:
let
  inherit (lib) concatLists imap1 length elem filter unique sort concatMapStringsSep
    hasAttr attrNames toInt mapAttrsToList;
  str = toString;
  nCages = length cages;
  # Ports per cage in the naming scheme = lanes of the widest cage (QSFP28: 4
  # lanes, at most 4 subports; QSFP-DD: 8).
  stride = lib.foldl' lib.max 1 (map length cages);
  parseAlias = alias: let m = builtins.match "Ethernet([0-9]+)/([0-9]+)" alias; in
    if m == null then null else { cage = toInt (lib.head m); sub = toInt (lib.last m); };
  laneIndex = cage: lane: let l = lib.elemAt cages (cage - 1); i = lib.lists.findFirstIndex (x: x == lane) null l; in
    if i == null then stride else i;   # foreign lane: an assertion fails anyway
  mkPort = cage: sub: lanes: speed: fromBreakout: {
    name = "Ethernet${str (stride * (cage - 1) + (if lanes == [ ] then 0 else laneIndex cage (lib.head lanes)))}";
    alias = "Ethernet${str cage}/${str sub}";
    inherit cage sub lanes speed fromBreakout;
  };
  cagePorts = cage: lanes:
    if hasAttr (str cage) breakout then
      map (p: let a = parseAlias p.alias; in
        mkPort cage (if a == null then 0 else a.sub) p.lanes p.speed true // { declaredAlias = p.alias; })
        breakout.${str cage}
    else [ (mkPort cage 1 lanes defaultSpeed false) ];
  ports = concatLists (imap1 cagePorts cages);

  dups = l: unique (filter (x: length (filter (y: y == x) l) > 1) l);
  showLanes = l: concatMapStringsSep "," str l;

  checkCage = key: list:
    let
      cageOk = builtins.match "[0-9]+" key != null && toInt key >= 1 && toInt key <= nCages;
      cage = toInt key;
      cageLanes = lib.elemAt cages (cage - 1);
      where = "services.sonic.breakout.\"${key}\"";
      used = concatLists (map (p: p.lanes) list);
      aliases = map (p: p.alias) list;
    in if !cageOk then [{
        assertion = false;
        message = "${where}: no such cage (this board has cages 1..${str nCages})";
      }] else [
      { assertion = list != [ ];
        message = "${where}: an override must list every resulting port (alias, lanes, speed); remove it for the 1x${str (defaultSpeed / 1000)}G default"; }
      { assertion = dups aliases == [ ];
        message = "${where}: port names used twice: ${lib.concatStringsSep " " (dups aliases)}"; }
      { assertion = dups used == [ ];
        message = "${where}: lanes used more than once: ${showLanes (dups used)} (each lane of the cage exactly once)"; }
      { assertion = filter (l: !elem l cageLanes) used == [ ];
        message = "${where}: lanes ${showLanes (filter (l: !elem l cageLanes) used)} are not lanes of cage ${key} (${showLanes cageLanes})"; }
      { assertion = let firsts = map (p: if p.lanes == [ ] then 0 else laneIndex cage (lib.head p.lanes))
                                  (lib.sort (a: b: (parseAlias a.alias).sub or 0 < (parseAlias b.alias).sub or 0) list);
                    in firsts == lib.sort (a: b: a < b) firsts;
        message = "${where}: subports must follow the lane order (Ethernet${key}/1 on the lowest lanes)"; }
      { assertion = let unused = filter (l: !elem l used) cageLanes; in
          unused == [ ] || (allowUnusedLanes && lib.drop (length cageLanes - length unused) cageLanes == unused);
        message = "${where}: lanes ${showLanes (filter (l: !elem l used) cageLanes)} of cage ${key} are not assigned (left over from the default port): list every port"
          + lib.optionalString allowUnusedLanes " (only the cage's upper lanes may stay unused)"; }
    ] ++ concatLists (map (p:
      let a = parseAlias p.alias; n = length p.lanes; in [
        { assertion = a != null && a.cage == cage && a.sub >= 1 && a.sub <= stride;
          message = "${where}: port \"${p.alias}\": name must be Ethernet${key}/<1..${str stride}> (front-panel alias)"; }
        { assertion = elem p.speed (validSpeeds.${str n} or [ ]);
          message = "${where}: port \"${p.alias}\": speed ${str p.speed} is not valid for ${str n} lane(s) (valid: ${let v = validSpeeds.${str n} or [ ]; in if v == [ ] then "none" else lib.concatMapStringsSep ", " str v})"; }
      ]) list);

  names = map (p: p.name) ports;
in {
  inherit ports parseAlias;
  aliasToName = builtins.listToAttrs (map (p: { name = p.alias; value = p.name; }) ports);
  assertions = concatLists (mapAttrsToList checkCage breakout) ++ [
    { assertion = dups names == [ ];
      message = "services.sonic: generated SONiC port names collide: ${lib.concatStringsSep " " (dups names)}"; }
    { assertion = dups (concatLists (map (p: p.lanes) ports)) == [ ];
      message = "services.sonic.cages: lanes ${showLanes (dups (concatLists (map (p: p.lanes) ports)))} belong to more than one port"; }
  ];
}
