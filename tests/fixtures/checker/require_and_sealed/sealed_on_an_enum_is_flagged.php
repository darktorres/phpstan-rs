<?php
            interface Shape {}
            /** @phpstan-sealed Shape */
            enum Color implements Shape { case Red; case Blue; }
        