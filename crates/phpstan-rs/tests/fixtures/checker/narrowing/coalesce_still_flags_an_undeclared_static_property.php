<?php
            class Bar {}
            function f(): string { return Bar::$missing ?? "default"; }
        