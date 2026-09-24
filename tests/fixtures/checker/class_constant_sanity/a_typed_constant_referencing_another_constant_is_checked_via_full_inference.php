<?php
            class Foo {
                const int BASE = 1;
                const int DERIVED = self::BASE;
                const string WRONG = self::BASE;
            }
        