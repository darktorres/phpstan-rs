<?php
            enum Suit {
                case Hearts;
                public function label(): string { return "Hearts"; }
            }
            function f(): void {
                Suit::Hearts->label();
                Suit::Hearts->missing();
            }
        