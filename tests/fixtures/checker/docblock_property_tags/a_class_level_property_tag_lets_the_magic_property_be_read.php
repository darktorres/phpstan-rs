<?php
            class Paths {}
            /**
             * @property Paths $paths
             */
            class OpenApi {}
            function f(OpenApi $o): Paths { return $o->paths; }
        