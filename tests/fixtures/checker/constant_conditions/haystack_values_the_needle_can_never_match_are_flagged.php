<?php
            enum Suit { case Hearts; case Spades; }
            enum Color { case Red; }
            function f(Suit $s, string $name, int $n): void {
                in_array($s, [Suit::Hearts, Color::Red], true);
                in_array($s, [Suit::Hearts, Suit::Spades], true);
                array_search($name, ['a', 1], true);
                in_array($n, ['x', 'y'], true);
                array_keys(['a' => 1], 'one', true);
            }
        