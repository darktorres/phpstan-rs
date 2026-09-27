#!/usr/bin/env python3
"""Runs real PHPStan and phpstan-rs over the same inputs and reports where
they differ: the first level each reports a (file, identifier) at, what
bleedingEdge adds, PHPStan's own rule fixtures per feature toggle, and each
parameter flipped away from its default with phpstan-rs checked on exactly
the lines the flip changes.

    gates/phpstan-oracle/oracle.py --phar <phpstan.phar> --rs <phpstan-rs> --out <dir>

The phar must sit outside any `vendor/` directory: inside one it loads that
project's autoloader, and the extension installer's extensions with it.
"""

import argparse
import collections
import json
import os
import re
import shutil
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
PROBES = os.path.join(HERE, "probes")
TESTS = os.path.join(ROOT, "vendor/phpstan-src/tests/PHPStan")

# Feature toggles whose own rule fixtures settle what phpstan-rs does.
TOGGLE_FIXTURES = {
    "checkDynamicConstantNameValues": ["Rules/Constants/data/value-assigned-to-define.php"],
    "checkParameterCastableToNumberFunctions": ["Rules/Functions/data/param-castable-to-number-functions.php"],
    "assignToByRefForeachExpr": ["Rules/Variables/data/assign-to-by-ref-expr-from-foreach.php"],
    "finiteTypesInHaystack": ["Rules/Comparison/data/impossible-in-array-finite-types.php"],
    "checkPrintfParameterTypes": ["Rules/Functions/data/printf-param-types.php"],
    "reportMethodPurityOverride": ["Rules/Methods/data/method-signature-pure-unless-callable.php"],
    "unnecessaryNullCoalesce": ["Rules/Variables/data/bug-4337.php"],
    "checkImportedClassNameCase": ["Rules/Methods/data/bug-12827.php"],
    "sortWithoutEffect": [
        "Rules/Functions/data/sort-without-effect.php",
        "Rules/Functions/data/sort-without-effect-named-args.php",
    ],
}

# Settings a toggle's fixture needs, as a PHPStan parameters block and the
# matching phpstan-rs `--set` assignments.
TOGGLE_SETTINGS = {
    "checkDynamicConstantNameValues": (
        "\tdynamicConstantNames:\n"
        "\t\tBAR_CONSTANT: 'int|string|null'\n"
        "\t\tDYNAMIC_INT_CONSTANT: 'int'\n"
        "\t\tMAYBE_CONSTANT: 'positive-int'\n"
        "\t\tA_NON_EMPTY_STRING: 'non-empty-string'\n",
        ['dynamic_constant_names={BAR_CONSTANT="int|string|null", DYNAMIC_INT_CONSTANT="int", '
         'MAYBE_CONSTANT="positive-int", A_NON_EMPTY_STRING="non-empty-string"}'],
    ),
}

# Each parameter flipped away from its default, with the PHPStan tests
# whose fixtures exercise it where the probes don't.
FLIPS = {
    "checkExplicitMixedMissingReturn: true": ["Rules/Missing/MissingReturnRuleTest.php"],
    "inferPrivatePropertyTypeFromConstructor: true": [],
    "reportMaybesInMethodSignatures: true": [],
    "reportMaybesInPropertyPhpDocTypes: true": [],
    "reportStaticMethodSignatures: true": [],
    "reportWrongPhpDocTypeInVarTag: true": [
        "Rules/PhpDoc/VarTagChangedExpressionTypeRuleTest.php",
        "Rules/PhpDoc/WrongVariableNameInVarTagRuleTest.php",
    ],
    "reportAnyTypeWideningInVarTag: true": [
        "Rules/PhpDoc/VarTagChangedExpressionTypeRuleTest.php",
        "Rules/PhpDoc/WrongVariableNameInVarTagRuleTest.php",
    ],
    "reportNonIntStringArrayKey: true": [],
    "reportPossiblyNonexistentGeneralArrayOffset: true": [],
    "reportPossiblyNonexistentConstantArrayOffset: true": [
        "Rules/Arrays/NonexistentOffsetInArrayDimFetchRuleTest.php"
    ],
    "checkMissingOverrideMethodAttribute: true": [],
    "checkMissingOverridePropertyAttribute: true": [],
    "reportAlwaysTrueInLastCondition: true": [],
    "reportUnsafeArrayStringKeyCasting: detect": [],
    "usePathConstantsAsConstantString: true": [],
    "checkInternalClassCaseSensitivity: true": [],
    "checkDynamicProperties: true": [
        "Rules/Properties/AccessPropertiesRuleTest.php",
        "Analyser/AnalyserWithCheckDynamicPropertiesTest.php",
    ],
    "cliArgumentsVariablesRegistered: true": ["Rules/Variables/DefinedVariableRuleTest.php"],
    "polluteScopeWithLoopInitialAssignments: false": [],
    "polluteScopeWithAlwaysIterableForeach: false": [],
    "polluteScopeWithBlock: false": [],
    "treatPhpDocTypesAsCertain: false": [],
    "rememberPossiblyImpureFunctionValues: false": [
        "Analyser/DoNotRememberPossiblyImpureFunctionValuesTest.php",
        "Rules/Comparison/ImpossibleCheckTypeFunctionCallRuleTest.php",
    ],
    "reportMagicMethods: false": [],
}


def phpstan(phar, workdir, paths, level, extra="", bleeding_edge=False):
    includes = f"includes:\n\t- phar://{phar}/conf/bleedingEdge.neon\n" if bleeding_edge else ""
    listed = "".join(f"\t\t- {p}\n" for p in paths)
    config = os.path.join(workdir, f"phpstan-{level}-{abs(hash(extra))}-{bleeding_edge}.neon")
    with open(config, "w") as f:
        f.write(f"{includes}parameters:\n\tlevel: {level}\n\tpaths:\n{listed}"
                f"\ttmpDir: {workdir}/tmp\n{extra}")
    out = subprocess.run(
        ["php", "-d", "memory_limit=3G", phar, "analyse", "-c", config,
         "--no-progress", "--error-format=json"],
        capture_output=True, text=True).stdout
    return findings(json.loads(out))


def phpstan_rs(binary, paths, level, sets=()):
    args = [binary, "check-project", "--no-composer", "--level", str(level), "--format", "json"]
    for s in sets:
        args += ["--set", s]
    out = subprocess.run(args + list(paths), capture_output=True, text=True).stdout
    return findings(json.loads(out))


def findings(report):
    counts = collections.Counter()
    for path, f in report["files"].items():
        for m in f["messages"]:
            counts[(os.path.basename(path), m["line"], m.get("identifier"))] += 1
    return counts


def by_identifier(counts):
    out = collections.Counter()
    for (name, _, identifier), n in counts.items():
        out[(name, identifier)] += n
    return out


def first_levels(runs):
    first = {}
    for level, counts in enumerate(runs):
        for key in by_identifier(counts):
            first.setdefault(key, level)
    return first


def fixture_data(test):
    source = open(os.path.join(TESTS, test)).read()
    base = os.path.dirname(os.path.join(TESTS, test))
    files = {os.path.normpath(base + rel) for rel in re.findall(r"__DIR__ \. '(/[^']+\.php)'", source)}
    return sorted(f for f in files if os.path.exists(f))


def staged(workdir, name, files):
    target = os.path.join(workdir, name)
    os.makedirs(target, exist_ok=True)
    for f in files:
        shutil.copy(f, os.path.join(target, os.path.basename(f)))
    return target


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--phar", required=True)
    parser.add_argument("--rs", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    phar = os.path.abspath(args.phar)
    os.makedirs(args.out, exist_ok=True)
    work = os.path.abspath(args.out)

    print("== first level reported, PHPStan / phpstan-rs")
    ps = first_levels([phpstan(phar, work, [PROBES], level) for level in range(11)])
    rs = first_levels([phpstan_rs(args.rs, [PROBES], level) for level in range(11)])
    for key in sorted(set(ps) | set(rs), key=lambda k: (k[0], str(k[1]))):
        mark = "" if ps.get(key) == rs.get(key) else "  <<"
        print(f"{key[0]:44} {str(key[1]):40} {ps.get(key, '-')!s:>3} / {rs.get(key, '-')!s:<3}{mark}")

    print("\n== what bleedingEdge adds at level 10")
    base = by_identifier(phpstan(phar, work, [PROBES], 10))
    edge = by_identifier(phpstan(phar, work, [PROBES], 10, bleeding_edge=True))
    for key in sorted(k for k in edge if edge[k] > base.get(k, 0)):
        print(f"  {key}")

    print("\n== feature-toggle fixtures: findings without / with bleedingEdge / phpstan-rs")
    for toggle, rel in TOGGLE_FIXTURES.items():
        target = staged(work, "toggle-" + toggle, [os.path.join(TESTS, r) for r in rel])
        extra, sets = TOGGLE_SETTINGS.get(toggle, ("", []))
        off = by_identifier(phpstan(phar, work, [target], 10, extra=extra))
        on = by_identifier(phpstan(phar, work, [target], 10, extra=extra, bleeding_edge=True))
        ours = by_identifier(phpstan_rs(args.rs, [target], 10, sets))
        for key in sorted(set(off) | set(on) | set(ours), key=str):
            print(f"  {toggle:40} {str(key[1]):42} {off.get(key, 0):3} {on.get(key, 0):3} {ours.get(key, 0):3}")

    print("\n== flips: lines the flip changes, and which setting phpstan-rs agrees with")
    for spec, tests in FLIPS.items():
        name = spec.split(":")[0]
        paths = [PROBES]
        if tests:
            paths.append(staged(work, "flip-" + name, [f for t in tests for f in fixture_data(t)]))
        default = phpstan(phar, work, paths, 10)
        flipped = phpstan(phar, work, paths, 10, extra=f"\t{spec}\n")
        ours = phpstan_rs(args.rs, paths, 10)
        added = [k for k in flipped if flipped[k] > default.get(k, 0)]
        removed = [k for k in default if default[k] > flipped.get(k, 0)]
        if not added and not removed:
            print(f"  {name}: the flip changes nothing - not verified")
            continue
        at_default = sum(k not in ours for k in added) + sum(k in ours for k in removed)
        print(f"  {name}: {len(added) + len(removed)} lines change; phpstan-rs agrees with "
              f"the default on {at_default}, with the flip on {len(added) + len(removed) - at_default}")


if __name__ == "__main__":
    main()
