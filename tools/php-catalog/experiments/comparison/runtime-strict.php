<?php
declare(strict_types=1);

$events = [];
$value = new class($events) implements Stringable {
    private array $events;
    public function __construct(array &$events) { $this->events =& $events; }
    public function __toString(): string {
        $this->events[] = 'cast';
        return 'abc';
    }
};

try {
    strlen($value);
    throw new LogicException('strict strlen unexpectedly accepted Stringable');
} catch (TypeError $error) {
    $events[] = 'type-error';
}

if ($events !== ['type-error']) {
    throw new LogicException('strict strlen invoked __toString before rejecting Stringable');
}

echo "strict-stringable-rejected-without-cast\n";
