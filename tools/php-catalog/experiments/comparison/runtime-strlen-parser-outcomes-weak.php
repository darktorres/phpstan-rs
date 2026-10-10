<?php
$events = [];
$expectedMessage = 'strlen(): Passing null to parameter #1 ($string) of type string is deprecated';
set_error_handler(static function (int $severity, string $message) use (&$events, $expectedMessage): bool {
    if ($severity !== E_DEPRECATED || $message !== $expectedMessage) {
        return false;
    }
    $events[] = 'handler:E_DEPRECATED';
    throw new RuntimeException('handler-throw');
});

try {
    strlen(null);
    $events[] = 'direct-null:returned';
} catch (RuntimeException $error) {
    $events[] = 'direct-null:caught:' . $error->getMessage();
}

$strlen = 'strlen';
try {
    $strlen(null);
    $events[] = 'dynamic-null:returned';
} catch (RuntimeException $error) {
    $events[] = 'dynamic-null:caught:' . $error->getMessage();
}
restore_error_handler();

try {
    strlen([]);
    $events[] = 'direct-array:returned';
} catch (TypeError $error) {
    $events[] = 'direct-array:TypeError';
}
try {
    $strlen([]);
    $events[] = 'dynamic-array:returned';
} catch (TypeError $error) {
    $events[] = 'dynamic-array:TypeError';
}

$expected = [
    'handler:E_DEPRECATED', 'direct-null:caught:handler-throw',
    'handler:E_DEPRECATED', 'dynamic-null:caught:handler-throw',
    'direct-array:TypeError', 'dynamic-array:TypeError',
];
if ($events !== $expected) {
    throw new RuntimeException(json_encode($events));
}
echo json_encode(['events' => $events]), "\n";
