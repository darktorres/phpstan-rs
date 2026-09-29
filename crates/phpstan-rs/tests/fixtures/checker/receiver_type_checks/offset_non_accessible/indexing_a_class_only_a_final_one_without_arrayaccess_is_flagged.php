<?php
                class Plain {}
                final class Sealed {}
                function f(Plain $p, Sealed $s) {
                    echo $p['a'];
                    echo $s['a'];
                }
