<?php
// Lists what the running PHP declares, for the catalog's check of its
// registrations: each internal function, each internal class, interface,
// trait and enum, and each method a class declares itself, one per line
// as tab-separated fields (`function`, name; `class`, name; `method`,
// class, name).

$lines = [];
foreach (get_defined_functions()['internal'] as $function) {
    $lines[] = "function\t" . (new ReflectionFunction($function))->getName();
}
$classes = array_merge(get_declared_classes(), get_declared_interfaces(), get_declared_traits());
foreach ($classes as $name) {
    $class = new ReflectionClass($name);
    if (!$class->isInternal()) {
        continue;
    }
    $lines[] = "class\t" . $class->getName();
    foreach ($class->getMethods() as $method) {
        if ($method->getDeclaringClass()->getName() === $class->getName()) {
            $lines[] = "method\t" . $class->getName() . "\t" . $method->getName();
        }
    }
}
sort($lines, SORT_STRING);
echo implode("\n", array_unique($lines)), "\n";
