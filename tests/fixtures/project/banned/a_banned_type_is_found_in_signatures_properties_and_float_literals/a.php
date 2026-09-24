<?php
final class Money {
    /** @var float */
    public $amount = 0;
    public function __construct(public float $rate = 1.5) {}
    /** @return list<float> */
    public function parts(): array { return [(float) '1']; }
}
