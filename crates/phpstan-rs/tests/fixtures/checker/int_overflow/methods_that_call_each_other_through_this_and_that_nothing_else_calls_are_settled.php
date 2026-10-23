<?php
declare(strict_types=1);

final class Journal
{
    private int $depth = 0;

    public function add(int $level, string $message): bool
    {
        $depth = ++$this->depth;
        if ($depth === 3) {
            $this->warning('a loop was stopped');
            return false;
        }
        echo $level, ' ', $message, "\n";
        $this->depth--;
        return true;
    }

    public function warning(string $message): void
    {
        $this->add(300, $message);
    }
}
