<?php
namespace CallEffects\ByRefBuiltins;

use function PHPStan\Testing\assertType;

final class Flag
{
    public static ?int $value = null;
}

final class Resets
{
    public function __destruct()
    {
        Flag::$value = 5;
    }
}

final class Quiet
{
    public function __destruct()
    {
    }
}

/** @param array<int, mixed> $list */
function sort(array &$list): void
{
    $list = [new Resets()];
}

function aSortedArrayOfQuietObjects(): void
{
    $l = [new Quiet(), new Quiet()]; \sort($l);
}

function theCallerSees_aSortedArrayOfQuietObjects(): void
{
    Flag::$value = null;
    aSortedArrayOfQuietObjects();
    assertType('null', Flag::$value);
}

function aSortedArrayOfLoudObjects(): void
{
    $l = [new Resets()]; \sort($l);
}

function theCallerSees_aSortedArrayOfLoudObjects(): void
{
    Flag::$value = null;
    aSortedArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aShuffledArrayOfQuietObjects(): void
{
    $l = [new Quiet()]; shuffle($l);
}

function theCallerSees_aShuffledArrayOfQuietObjects(): void
{
    Flag::$value = null;
    aShuffledArrayOfQuietObjects();
    assertType('null', Flag::$value);
}

function anArrayPopOfLoudObjects(): void
{
    $l = [new Resets()]; array_pop($l);
}

function theCallerSees_anArrayPopOfLoudObjects(): void
{
    Flag::$value = null;
    anArrayPopOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aUserSortedArrayOfQuietObjects(): void
{
    $l = [new Quiet()]; usort($l, fn($a, $b) => 0);
}

function theCallerSees_aUserSortedArrayOfQuietObjects(): void
{
    Flag::$value = null;
    aUserSortedArrayOfQuietObjects();
    assertType('null', Flag::$value);
}

function aUserSortedArrayOfLoudObjects(): void
{
    $l = [new Resets()]; usort($l, fn($a, $b) => 0);
}

function theCallerSees_aUserSortedArrayOfLoudObjects(): void
{
    Flag::$value = null;
    aUserSortedArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aMatchIntoAnUndefinedVariable(): void
{
    preg_match('/x/', 'y', $m);
}

function theCallerSees_aMatchIntoAnUndefinedVariable(): void
{
    Flag::$value = null;
    aMatchIntoAnUndefinedVariable();
    assertType('null', Flag::$value);
}

function aMatchOverAnObject(): void
{
    $m = new Resets(); preg_match('/x/', 'y', $m);
}

function theCallerSees_aMatchOverAnObject(): void
{
    Flag::$value = null;
    aMatchOverAnObject();
    assertType('int|null', Flag::$value);
}

function aMatchOverAnArrayOfQuietObjects(): void
{
    $m = [new Quiet()]; preg_match('/x/', 'y', $m);
}

function theCallerSees_aMatchOverAnArrayOfQuietObjects(): void
{
    Flag::$value = null;
    aMatchOverAnArrayOfQuietObjects();
    assertType('null', Flag::$value);
}

function aMatchOverAnArrayOfLoudObjects(): void
{
    $m = [new Resets()]; preg_match('/x/', 'y', $m);
}

function theCallerSees_aMatchOverAnArrayOfLoudObjects(): void
{
    Flag::$value = null;
    aMatchOverAnArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aSplicedArrayOfLoudObjects(): void
{
    $l = [1]; array_splice($l, 0, 0, [new Resets()]);
}

function theCallerSees_aSplicedArrayOfLoudObjects(): void
{
    Flag::$value = null;
    aSplicedArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aSplicedObject(): void
{
    $l = [1]; array_splice($l, 0, 0, new Resets());
}

function theCallerSees_aSplicedObject(): void
{
    Flag::$value = null;
    aSplicedObject();
    assertType('int|null', Flag::$value);
}

function aReplaceCount(): void
{
    str_replace('a', 'b', 'c', $count);
}

function theCallerSees_aReplaceCount(): void
{
    Flag::$value = null;
    aReplaceCount();
    assertType('null', Flag::$value);
}

function aSettypeToInt(): void
{
    $v = new Resets(); settype($v, 'int');
}

function theCallerSees_aSettypeToInt(): void
{
    Flag::$value = null;
    aSettypeToInt();
    assertType('int|null', Flag::$value);
}

function aSettypeOfQuietArrayToString(): void
{
    $v = [new Quiet()]; settype($v, 'string');
}

function theCallerSees_aSettypeOfQuietArrayToString(): void
{
    Flag::$value = null;
    aSettypeOfQuietArrayToString();
    assertType('null', Flag::$value);
}

function anExecOutputAppended(): void
{
    $o = [new Resets()]; exec('x', $o);
}

function theCallerSees_anExecOutputAppended(): void
{
    Flag::$value = null;
    anExecOutputAppended();
    assertType('int|null', Flag::$value);
}

function anExecOutputOfQuietObjects(): void
{
    $o = [new Quiet()]; exec('x', $o, $code);
}

function theCallerSees_anExecOutputOfQuietObjects(): void
{
    Flag::$value = null;
    anExecOutputOfQuietObjects();
    assertType('null', Flag::$value);
}

function aScanOverAnObject(): void
{
    $a = new Resets(); sscanf('1 2', '%d %d', $a, $b);
}

function theCallerSees_aScanOverAnObject(): void
{
    Flag::$value = null;
    aScanOverAnObject();
    assertType('int|null', Flag::$value);
}

/** @param array<int, mixed> $out */
function aByReferenceParameterIsMatchedOver(array &$out): void
{
    preg_match('/x/', 'y', $out);
}

function theCallerSeesNothingRunForQuietObjectsMatchedOver(): void
{
    $kept = [new Quiet()];
    Flag::$value = null;
    aByReferenceParameterIsMatchedOver($kept);
    assertType('null', Flag::$value);
}

function aMultisortOfQuietObjects(): void
{
    $a = [new Quiet()]; $b = [1]; array_multisort($a, SORT_DESC, $b);
}

function theCallerSees_aMultisortOfQuietObjects(): void
{
    Flag::$value = null;
    aMultisortOfQuietObjects();
    assertType('null', Flag::$value);
}

function aMultisortOfLoudObjects(): void
{
    $a = [new Resets()]; $b = [1]; array_multisort($a, SORT_DESC, $b);
}

function theCallerSees_aMultisortOfLoudObjects(): void
{
    Flag::$value = null;
    aMultisortOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function anIsCallableNameOverQuietObjects(): void
{
    $n = [new Quiet()]; is_callable('strlen', false, $n);
}

function theCallerSees_anIsCallableNameOverQuietObjects(): void
{
    Flag::$value = null;
    anIsCallableNameOverQuietObjects();
    assertType('null', Flag::$value);
}

function aSystemResultOverQuietObjects(): void
{
    $c = [new Quiet()]; system('x', $c);
}

function theCallerSees_aSystemResultOverQuietObjects(): void
{
    Flag::$value = null;
    aSystemResultOverQuietObjects();
    assertType('null', Flag::$value);
}

function aPassthruResultOverQuietObjects(): void
{
    $c = [new Quiet()]; passthru('x', $c);
}

function theCallerSees_aPassthruResultOverQuietObjects(): void
{
    Flag::$value = null;
    aPassthruResultOverQuietObjects();
    assertType('null', Flag::$value);
}

function aSimilarTextPercentOverQuietObjects(): void
{
    $p = [new Quiet()]; similar_text('a', 'b', $p);
}

function theCallerSees_aSimilarTextPercentOverQuietObjects(): void
{
    Flag::$value = null;
    aSimilarTextPercentOverQuietObjects();
    assertType('null', Flag::$value);
}

function aParseStrResultOverQuietObjects(): void
{
    $r = [new Quiet()]; parse_str('a=1', $r);
}

function theCallerSees_aParseStrResultOverQuietObjects(): void
{
    Flag::$value = null;
    aParseStrResultOverQuietObjects();
    assertType('null', Flag::$value);
}

function aMatchAllWithFlagsOverQuietObjects(): void
{
    $m = [new Quiet()]; preg_match_all('/x/', 'y', $m, PREG_SET_ORDER | PREG_OFFSET_CAPTURE);
}

function theCallerSees_aMatchAllWithFlagsOverQuietObjects(): void
{
    Flag::$value = null;
    aMatchAllWithFlagsOverQuietObjects();
    assertType('null', Flag::$value);
}

function aMatchWithUnmatchedAsNullOverLoudObjects(): void
{
    $m = [new Resets()]; preg_match('/x/', 'y', $m, PREG_UNMATCHED_AS_NULL);
}

function theCallerSees_aMatchWithUnmatchedAsNullOverLoudObjects(): void
{
    Flag::$value = null;
    aMatchWithUnmatchedAsNullOverLoudObjects();
    assertType('int|null', Flag::$value);
}

function aReplaceCountOverQuietObjects(): void
{
    $c = [new Quiet()]; preg_replace('/x/', 'y', 'z', -1, $c);
}

function theCallerSees_aReplaceCountOverQuietObjects(): void
{
    Flag::$value = null;
    aReplaceCountOverQuietObjects();
    assertType('null', Flag::$value);
}

function aStrReplaceCountOverQuietObjects(): void
{
    $c = [new Quiet()]; str_replace('a', 'b', 'c', $c);
}

function theCallerSees_aStrReplaceCountOverQuietObjects(): void
{
    Flag::$value = null;
    aStrReplaceCountOverQuietObjects();
    assertType('null', Flag::$value);
}

function aSettypeOfLoudArrayToBool(): void
{
    $v = [new Resets()]; settype($v, 'bool');
}

function theCallerSees_aSettypeOfLoudArrayToBool(): void
{
    Flag::$value = null;
    aSettypeOfLoudArrayToBool();
    assertType('int|null', Flag::$value);
}

function anExecResultCodeOverQuietObjects(): void
{
    $c = [new Quiet()]; exec('x', $o, $c);
}

function theCallerSees_anExecResultCodeOverQuietObjects(): void
{
    Flag::$value = null;
    anExecResultCodeOverQuietObjects();
    assertType('null', Flag::$value);
}

function anArrayPopOfQuietObjects(): void
{
    $l = [new Quiet()]; array_pop($l);
}

function theCallerSees_anArrayPopOfQuietObjects(): void
{
    Flag::$value = null;
    anArrayPopOfQuietObjects();
    assertType('null', Flag::$value);
}

function anArrayPopOfLoudObjects(): void
{
    $l = [new Resets()]; array_pop($l);
}

function theCallerSees_anArrayPopOfLoudObjects(): void
{
    Flag::$value = null;
    anArrayPopOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function anArrayPopKeptInAVariableOfQuietObjects(): void
{
    $l = [new Quiet()]; $x = array_pop($l);
}

function theCallerSees_anArrayPopKeptInAVariableOfQuietObjects(): void
{
    Flag::$value = null;
    anArrayPopKeptInAVariableOfQuietObjects();
    assertType('null', Flag::$value);
}

function anArrayShiftOfQuietObjects(): void
{
    $l = [new Quiet()]; array_shift($l);
}

function theCallerSees_anArrayShiftOfQuietObjects(): void
{
    Flag::$value = null;
    anArrayShiftOfQuietObjects();
    assertType('null', Flag::$value);
}

function anArrayShiftOfNumbers(): void
{
    $l = [1, 2]; array_shift($l);
}

function theCallerSees_anArrayShiftOfNumbers(): void
{
    Flag::$value = null;
    anArrayShiftOfNumbers();
    assertType('null', Flag::$value);
}

function anEndOfQuietObjects(): void
{
    $l = [new Quiet()]; end($l);
}

function theCallerSees_anEndOfQuietObjects(): void
{
    Flag::$value = null;
    anEndOfQuietObjects();
    assertType('null', Flag::$value);
}

function aResetOfQuietObjects(): void
{
    $l = [new Quiet()]; reset($l);
}

function theCallerSees_aResetOfQuietObjects(): void
{
    Flag::$value = null;
    aResetOfQuietObjects();
    assertType('null', Flag::$value);
}

function aNextOfQuietObjects(): void
{
    $l = [new Quiet()]; next($l); prev($l); current($l);
}

function theCallerSees_aNextOfQuietObjects(): void
{
    Flag::$value = null;
    aNextOfQuietObjects();
    assertType('null', Flag::$value);
}

function aSpliceOfQuietObjects(): void
{
    $l = [new Quiet(), 1]; array_splice($l, 0, 1);
}

function theCallerSees_aSpliceOfQuietObjects(): void
{
    Flag::$value = null;
    aSpliceOfQuietObjects();
    assertType('null', Flag::$value);
}

function aSpliceOfLoudObjects(): void
{
    $l = [new Resets(), 1]; array_splice($l, 0, 1);
}

function theCallerSees_aSpliceOfLoudObjects(): void
{
    Flag::$value = null;
    aSpliceOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function aPopOfNestedQuietArrays(): void
{
    $l = [[new Quiet()]]; array_pop($l);
}

function theCallerSees_aPopOfNestedQuietArrays(): void
{
    Flag::$value = null;
    aPopOfNestedQuietArrays();
    assertType('null', Flag::$value);
}

function aPopOfNestedLoudArrays(): void
{
    $l = [[new Resets()]]; array_pop($l);
}

function theCallerSees_aPopOfNestedLoudArrays(): void
{
    Flag::$value = null;
    aPopOfNestedLoudArrays();
    assertType('int|null', Flag::$value);
}

function aPopInALoopThatAppendsLoudObjects(): void
{
    $l = []; for ($i = 0; $i < 2; $i++) { array_pop($l); $l[] = new Resets(); }
}

function theCallerSees_aPopInALoopThatAppendsLoudObjects(): void
{
    Flag::$value = null;
    aPopInALoopThatAppendsLoudObjects();
    assertType('int|null', Flag::$value);
}

function aPopInALoopThatAppendsQuietObjects(): void
{
    $l = []; for ($i = 0; $i < 2; $i++) { array_pop($l); $l[] = new Quiet(); }
}

function theCallerSees_aPopInALoopThatAppendsQuietObjects(): void
{
    Flag::$value = null;
    aPopInALoopThatAppendsQuietObjects();
    assertType('null', Flag::$value);
}

function aPopOfAnArrayFilledLater(): void
{
    $l = []; array_pop($l); $l[] = new Quiet();
}

function theCallerSees_aPopOfAnArrayFilledLater(): void
{
    Flag::$value = null;
    aPopOfAnArrayFilledLater();
    assertType('null', Flag::$value);
}

function aWalkOverQuietObjects(): void
{
    $l = [new Quiet()]; array_walk($l, function ($x) {});
}

function theCallerSees_aWalkOverQuietObjects(): void
{
    Flag::$value = null;
    aWalkOverQuietObjects();
    assertType('null', Flag::$value);
}

function aWalkOverLoudObjects(): void
{
    $l = [new Resets()]; array_walk($l, function ($x) {});
}

function theCallerSees_aWalkOverLoudObjects(): void
{
    Flag::$value = null;
    aWalkOverLoudObjects();
    assertType('int|null', Flag::$value);
}

function aRecursiveWalkOverQuietObjects(): void
{
    $l = [[new Quiet()]]; array_walk_recursive($l, function ($x) {});
}

function theCallerSees_aRecursiveWalkOverQuietObjects(): void
{
    Flag::$value = null;
    aRecursiveWalkOverQuietObjects();
    assertType('null', Flag::$value);
}

function aWalkWithAByReferenceCallbackStoringLoudObjects(): void
{
    $l = [1]; array_walk($l, function (&$x) { $x = new Resets(); });
}

function theCallerSees_aWalkWithAByReferenceCallbackStoringLoudObjects(): void
{
    Flag::$value = null;
    aWalkWithAByReferenceCallbackStoringLoudObjects();
    assertType('int|null', Flag::$value);
}

function countOfAnArrayLiteralAfterQuietObjects(): void
{
    $l = [new Quiet()]; count([]); implode(',', []);
}

function theCallerSees_countOfAnArrayLiteralAfterQuietObjects(): void
{
    Flag::$value = null;
    countOfAnArrayLiteralAfterQuietObjects();
    assertType('null', Flag::$value);
}

function countOfAnArrayLiteralAfterLoudObjects(): void
{
    $l = [new Resets()]; count([]);
}

function theCallerSees_countOfAnArrayLiteralAfterLoudObjects(): void
{
    Flag::$value = null;
    countOfAnArrayLiteralAfterLoudObjects();
    assertType('int|null', Flag::$value);
}

function getoptWithAnArrayLiteral(): void
{
    $l = [new Quiet()]; getopt('a', ['b']);
}

function theCallerSees_getoptWithAnArrayLiteral(): void
{
    Flag::$value = null;
    getoptWithAnArrayLiteral();
    assertType('null', Flag::$value);
}

function sscanfResultAfterQuietObjects(): void
{
    $l = [new Quiet()]; $r = sscanf('1', '%d');
}

function theCallerSees_sscanfResultAfterQuietObjects(): void
{
    Flag::$value = null;
    sscanfResultAfterQuietObjects();
    assertType('null', Flag::$value);
}

function sscanfResultAfterLoudObjects(): void
{
    $l = [new Resets()]; $r = sscanf('1', '%d');
}

function theCallerSees_sscanfResultAfterLoudObjects(): void
{
    Flag::$value = null;
    sscanfResultAfterLoudObjects();
    assertType('int|null', Flag::$value);
}

function settypeToObjectOfQuietObjects(): void
{
    $v = [new Quiet()]; settype($v, 'object');
}

function theCallerSees_settypeToObjectOfQuietObjects(): void
{
    Flag::$value = null;
    settypeToObjectOfQuietObjects();
    assertType('null', Flag::$value);
}

function settypeToObjectOfLoudObjects(): void
{
    $v = [new Resets()]; settype($v, 'object');
}

function theCallerSees_settypeToObjectOfLoudObjects(): void
{
    Flag::$value = null;
    settypeToObjectOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function settypeToArrayOfLoudObjects(): void
{
    $v = [new Resets()]; settype($v, 'array');
}

function theCallerSees_settypeToArrayOfLoudObjects(): void
{
    Flag::$value = null;
    settypeToArrayOfLoudObjects();
    assertType('int|null', Flag::$value);
}

function settypeToObjectThenOverwritten(): void
{
    $v = [new Resets()]; settype($v, 'object'); $v = 1;
}

function theCallerSees_settypeToObjectThenOverwritten(): void
{
    Flag::$value = null;
    settypeToObjectThenOverwritten();
    assertType('int|null', Flag::$value);
}
