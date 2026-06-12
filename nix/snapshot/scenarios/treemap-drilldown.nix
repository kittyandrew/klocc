{
  id = "treemap-drilldown";
  title = "klocc GUI Walkthrough Review";
  label = "Treemap drilldown basic";
  description = "Root, two drilldowns, return to root, and open a gray tiny aggregate.";

  screen = {
    width = 1920;
    height = 1080;
  };

  app = {
    width = 1760;
    height = 940;
  };

  artifacts = {
    perStepExtensions = ["png" "stats" "rects"];
    common = [
      "walkthrough.png"
      "walkthrough.stats"
      "klocc-gui.log"
      "environment.log"
    ];
  };

  assertions = {
    artifact = {
      minSourceUnitsWithLoc = 100;
      minTotalCodeLoc = 1000000;
    };
    telemetry = {
      minLoadedSources = 100;
      minLayoutPasses = 3;
      minMaxRects = 100;
      minPaintLabels = 7;
      maxSlivers = 0;
      maxTiny = 0;
      requireNoZeroRectLayouts = true;
      requirePaintQuality = true;
    };
    images = {
      minStddev = 1000;
      requireSizeMatchesScreen = true;
      requireWalkthrough = true;
    };
  };

  steps = [
    {
      id = "current";
      title = "Current";
      label = "Root";
      description = "Root treemap with the largest source hovered.";
      actions = [
        {
          op = "move";
          point = [529 310];
        }
        {
          op = "sleep";
          seconds = 1;
        }
      ];
    }
    {
      id = "largest-top-left";
      title = "Largest Top Left";
      label = "First drilldown";
      description = "After drilling into the largest drillable source from the root view.";
      actions = [
        {
          op = "click";
          point = [529 310];
          button = "left";
        }
        {
          op = "sleep";
          seconds = 1;
        }
        {
          op = "move";
          point = [661 489];
        }
        {
          op = "sleep";
          seconds = 1;
        }
      ];
    }
    {
      id = "third-layer";
      title = "Third Layer";
      label = "Second drilldown";
      description = "After drilling once more into the largest source in the second-layer view.";
      actions = [
        {
          op = "click";
          point = [661 489];
          button = "left";
        }
        {
          op = "sleep";
          seconds = 1;
        }
        {
          op = "move";
          point = [974 489];
        }
        {
          op = "sleep";
          seconds = 1;
        }
      ];
    }
    {
      id = "gray-tiny";
      title = "Gray Tiny";
      label = "Tiny aggregate";
      description = "After returning to root and opening the largest gray tiny aggregate.";
      actions = [
        {
          op = "click";
          point = [880 470];
          button = "right";
        }
        {
          op = "sleep";
          seconds = 1;
        }
        {
          op = "click";
          point = [880 470];
          button = "right";
        }
        {
          op = "sleep";
          seconds = 1;
        }
        {
          op = "click";
          point = [1420 703];
          button = "left";
        }
        {
          op = "sleep";
          seconds = 2;
        }
        {
          op = "move";
          point = [974 489];
        }
        {
          op = "sleep";
          seconds = 1;
        }
      ];
    }
  ];
}
