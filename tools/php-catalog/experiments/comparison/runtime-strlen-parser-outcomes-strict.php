<?php
declare(strict_types=1);

$events = [];
set_error_handler(static function (int $severity, string $message) use (&$events): bool {
    $events[] = 'handler:' . $severity . ':' . $message;
    return true;
});

try {
    strlen(null);
    $events[] = 'direct-null:returned';
} catch (TypeError $error) {
    $events[] = 'direct-null:TypeError';
}

$strlen = 'strlen';
try {
    $strlen(null);
    $events[] = 'dynamic-null:returned';
} catch (TypeError $error) {
    $events[] = 'dynamic-null:TypeError';
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
    'direct-null:TypeError', 'dynamic-null:TypeError',
    'direct-array:TypeError', 'dynamic-array:TypeError',
];
if ($events !== $expected) {
    throw new RuntimeException(json_encode($events));
}
echo json_encode(['events' => $events]), "\n";
