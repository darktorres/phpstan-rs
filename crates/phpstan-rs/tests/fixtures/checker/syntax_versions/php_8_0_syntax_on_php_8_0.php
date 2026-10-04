<?php
class A
{
    public int|string $u = 1;

    public function f(mixed $x, ?DateTime $d,): static
    {
        try {
        } catch (Exception) {
        }
        $y = $x ?? throw new Exception();
        echo $this::class, $d?->format("Y"), match ($y) { default => 1 };
        return $this;
    }
}
