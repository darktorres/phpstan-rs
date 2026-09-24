<?php
            enum Suit { case Hearts; case Diamonds; case Clubs; case Spades; }
            function color(Suit $s): string {
                return match ($s) {
                    Suit::Hearts, Suit::Diamonds => "red",
                    Suit::Clubs => "black",
                };
            }
        