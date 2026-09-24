<?php
            class Foo {}
            function test(Foo $f): void {
                ~$f;
            }
        