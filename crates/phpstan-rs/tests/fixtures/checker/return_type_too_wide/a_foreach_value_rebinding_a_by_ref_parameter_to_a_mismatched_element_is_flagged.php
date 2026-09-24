<?php
            function f(int &$x): void {
                foreach (['not an int'] as $x) {
                }
            }
        