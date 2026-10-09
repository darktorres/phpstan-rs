<?php
declare(strict_types=1);
// Strict mode rejects weak scalar conversions before strlen() runs.
$types = [];
foreach ([123, 1.5, true, false, null] as $value) {
    try {
        strlen($value);
        throw new LogicException('strict strlen unexpectedly accepted a scalar');
    } catch (TypeError $error) {
        $types[] = get_debug_type($value);
    }
}
$expected = ['int', 'float', 'bool', 'bool', 'null'];
if ($types !== $expected) {
    throw new RuntimeException(json_encode($types));
}
echo json_encode(['rejected' => $types]), "\n";
