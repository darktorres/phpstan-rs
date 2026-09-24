<?php
            /** @method ?string getAgentInfo() */
            interface IBarPanel {
                function getTab();
            }
            class ConcretePanel implements IBarPanel {
                function getTab() { return "tab"; }
            }
        