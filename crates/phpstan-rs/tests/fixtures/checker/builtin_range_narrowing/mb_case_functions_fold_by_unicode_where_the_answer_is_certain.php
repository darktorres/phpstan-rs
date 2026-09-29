<?php
use function PHPStan\Testing\assertType;
assertType("'école'", mb_strtolower('ÉCOLE'));
assertType("'Ecole'", ucfirst('ecole'));
assertType("'STRASSE'", mb_strtoupper('straße'));
assertType("'Hello O\\'neil 3Rd'", mb_convert_case("hello o'neil 3rd", MB_CASE_TITLE));
assertType("'Élan-Vital'", mb_convert_case('élan-vital', MB_CASE_TITLE));
assertType("'ǅemal'", mb_ucfirst('ǆemal'));
assertType('non-falsy-string', mb_ucfirst('ß'));
assertType("'hello'", mb_convert_case('HELLO', MB_CASE_FOLD));
assertType('non-falsy-lowercase-string', mb_convert_case('Straße', MB_CASE_FOLD));
assertType("'éCOLE'", mb_lcfirst('ÉCOLE'));
