<?php
$events = [];
$writes = [];
$output = '';

final class StrlenReleaseProbe implements Stringable
{
    public function __construct(
        private string $name,
        private bool $reenter = false,
        private bool $emit = false,
    ) {}

    public function __toString(): string
    {
        $GLOBALS['events'][] = $this->name . ':cast';
        $GLOBALS['writes'][] = $this->name . ':write';
        return 'abc';
    }

    public function __destruct()
    {
        $GLOBALS['events'][] = $this->name . ':destruct';
        if ($this->emit) {
            echo $this->name . ';';
        }
        if ($this->reenter) {
            $GLOBALS['events'][] = 'outer:before-reentry';
            $inner = new class implements Stringable {
                public function __toString(): string
                {
                    $GLOBALS['events'][] = 'inner:cast';
                    return 'x';
                }

                public function __destruct()
                {
                    $GLOBALS['events'][] = 'inner:destruct';
                }
            };
            $GLOBALS['events'][] = 'inner:length:' . strlen($inner);
            unset($inner);
            $GLOBALS['events'][] = 'outer:after-reentry';
        }
    }
}

ob_start();
$scalarLength = strlen('abc');
$scalarEvents = $events;
$retained = new StrlenReleaseProbe('retained');
$retainedLength = strlen($retained);
$retainedEvents = $events;
unset($retained);
$afterRetainedUnset = $events;
$temporaryLength = strlen(new StrlenReleaseProbe('temporary', emit: true));
$afterTemporary = $events;
$callable = 'strlen';
$dynamicLength = $callable(new StrlenReleaseProbe('dynamic'));
$afterDynamic = $events;
try {
    strlen(new class implements Stringable {
        public function __toString(): string
        {
            $GLOBALS['events'][] = 'throwing:cast';
            $GLOBALS['writes'][] = 'throwing:write';
            throw new RuntimeException('expected');
        }

        public function __destruct()
        {
            $GLOBALS['events'][] = 'throwing:destruct';
        }
    });
    $events[] = 'throwing:after';
} catch (RuntimeException $e) {
    $events[] = 'throwing:caught';
}
$afterThrow = $events;
$reentrantLength = strlen(new StrlenReleaseProbe('reentrant', reenter: true));
$afterReentrant = $events;
$output = ob_get_clean();

$observed = compact(
    'scalarLength',
    'scalarEvents',
    'retainedLength',
    'retainedEvents',
    'afterRetainedUnset',
    'temporaryLength',
    'afterTemporary',
    'dynamicLength',
    'afterDynamic',
    'afterThrow',
    'reentrantLength',
    'afterReentrant',
    'writes',
    'output',
);
$expected = [
    'scalarLength' => 3,
    'scalarEvents' => [],
    'retainedLength' => 3,
    'retainedEvents' => ['retained:cast'],
    'afterRetainedUnset' => ['retained:cast', 'retained:destruct'],
    'temporaryLength' => 3,
    'afterTemporary' => ['retained:cast', 'retained:destruct', 'temporary:cast', 'temporary:destruct'],
    'dynamicLength' => 3,
    'afterDynamic' => ['retained:cast', 'retained:destruct', 'temporary:cast', 'temporary:destruct', 'dynamic:cast', 'dynamic:destruct'],
    'afterThrow' => ['retained:cast', 'retained:destruct', 'temporary:cast', 'temporary:destruct', 'dynamic:cast', 'dynamic:destruct', 'throwing:cast', 'throwing:destruct', 'throwing:caught'],
    'reentrantLength' => 3,
    'afterReentrant' => ['retained:cast', 'retained:destruct', 'temporary:cast', 'temporary:destruct', 'dynamic:cast', 'dynamic:destruct', 'throwing:cast', 'throwing:destruct', 'throwing:caught', 'reentrant:cast', 'reentrant:destruct', 'outer:before-reentry', 'inner:cast', 'inner:length:1', 'inner:destruct', 'outer:after-reentry'],
    'writes' => ['retained:write', 'temporary:write', 'dynamic:write', 'throwing:write', 'reentrant:write'],
    'output' => 'temporary;',
];
if ($observed !== $expected) {
    throw new RuntimeException(json_encode($observed, JSON_THROW_ON_ERROR));
}
echo json_encode($observed, JSON_THROW_ON_ERROR), "\n";
