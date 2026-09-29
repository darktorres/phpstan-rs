<?php
                class Plain {}
                final class Sealed {}
                function f(Plain $x, Sealed $y) {
                    $x();
                    $y();
                }
