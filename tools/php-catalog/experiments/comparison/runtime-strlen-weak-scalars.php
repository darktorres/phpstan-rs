<?php
// Runtime witnesses for zend_parse_arg_str_weak through the real strlen() API.
$deprecations = [];
set_error_handler(static function (int $severity, string $message) use (&$deprecations): bool {
    if ($severity !== E_DEPRECATED) {
        return false;
    }
    $deprecations[] = $message;
    return true;
});
$lengths = [strlen(123), strlen(1.5), strlen(true), strlen(false), strlen(null)];
restore_error_handler();
$expected = [3, 3, 1, 0, 0];
$expectedDeprecation = 'strlen(): Passing null to parameter #1 ($string) of type string is deprecated';
if ($lengths !== $expected || $deprecations !== [$expectedDeprecation]) {
    throw new RuntimeException(json_encode([$lengths, $deprecations]));
}
echo json_encode(['lengths' => $lengths, 'deprecations' => $deprecations]), "\n";
