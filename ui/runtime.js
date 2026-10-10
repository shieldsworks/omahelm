function runtimeDir(value) {
    if (typeof value !== "string" || value.length === 0 || value.charAt(0) !== "/")
        return "";
    var base = value;
    while (base.length > 1 && base.charAt(base.length - 1) === "/")
        base = base.slice(0, base.length - 1);
    if (base === "/")
        return "/omahelm/";
    return base + "/omahelm/";
}

function runtimeError(value) {
    if (runtimeDir(value) === "")
        return "XDG_RUNTIME_DIR must be set to an absolute path";
    return "";
}
